//! The virtual machine.
//!
//! Executes the bytecode from [`crate::bytecode`]. It is a second consumer of
//! the lowered form, which is what `SPEC.md` section 10.1 asks for: the
//! frontend lowers once and each execution engine walks the result.
//!
//! The VM exists to remove the cost a tree walk cannot avoid, which is deciding
//! what to do next. A tree walk matches on an expression node every time it
//! evaluates one. Here each expression is compiled once into instructions, and
//! the inner loop is a match on an enum followed by a jump, with no traversal
//! and no per node dispatch.
//!
//! Break and continue are a flag on the frame rather than jumps, so the compiler
//! never has to patch an unresolved jump when a loop closes.

use std::io::Write;

use crate::ast;
use crate::bytecode::{Code, Op, Target};
use crate::span::Pos;
use crate::value::{RuntimeError, Value};

/// How deeply calls may nest, so a runaway recursion reports rather than
/// exhausting the machine stack.
const MAX_DEPTH: usize = 800;

/// The stack a run gets, in bytes. The VM uses more native stack per Vortex
/// call than a tree walk does, so it gets a thread of its own.
const STACK_BYTES: usize = 64 * 1024 * 1024;

/// Runs a compiled program, writing whatever it prints to `out`.
///
/// The program must declare `main`. The value `main` returns is returned here.
pub fn run(program: &crate::bytecode::Program, out: &mut dyn Write) -> Result<Value, RuntimeError> {
    match run_collecting_output(program) {
        Ok((value, text)) => {
            out.write_all(text.as_bytes())
                .map_err(|e| RuntimeError::BadControlFlow {
                    pos: Pos::START,
                    detail: format!("could not write output: {}", e),
                })?;
            Ok(value)
        }
        Err(e) => Err(e),
    }
}

/// Runs a compiled program and returns its value together with its output.
pub fn run_collecting_output(
    program: &crate::bytecode::Program,
) -> Result<(Value, String), RuntimeError> {
    let program = std::sync::Arc::new(program.clone());
    let handle = std::thread::Builder::new()
        .name("vortex-vm".to_string())
        .stack_size(STACK_BYTES)
        .spawn(move || {
            let mut buffer: Vec<u8> = Vec::new();
            let result = {
                let mut vm = Vm::new(&mut buffer, &program);
                vm.call("main", Vec::new(), Pos::START)
            };
            (result, buffer)
        })
        .expect("the vm thread should start");

    match handle.join() {
        Ok((result, buffer)) => {
            let text = String::from_utf8(buffer).expect("output should be valid UTF-8");
            result.map(|v| (v, text))
        }
        Err(_) => Err(RuntimeError::BadControlFlow {
            pos: Pos::START,
            detail: "the vm thread panicked".into(),
        }),
    }
}

/// Why a loop body stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    Normal,
    Break,
    Continue,
}

/// A call frame.
///
/// The value stack is per frame rather than per call, so a loop inside a
/// function does not allocate. It grows as an expression nests and is dropped
/// when the frame returns.
struct Frame {
    slots: Vec<Value>,
    stack: Vec<Value>,
    ip: usize,
    flow: Flow,
    /// How many loop bodies this frame is inside, so a `Break` knows whether it
    /// has a loop to break out of.
    loops: usize,
    /// How many values the next `BuildList` gathers, carried by the `ListLen`
    /// the compiler emitted before it.
    pending_len: usize,
    /// The stack depth each open loop body started from.
    loop_bases: Vec<usize>,
}

impl Frame {
    fn new(size: usize) -> Frame {
        Frame {
            slots: vec![Value::Unit; size],
            stack: Vec::with_capacity(16),
            ip: 0,
            flow: Flow::Normal,
            loops: 0,
            pending_len: 0,
            loop_bases: Vec::new(),
        }
    }

    fn get(&self, slot: usize) -> Value {
        self.slots.get(slot).cloned().unwrap_or(Value::Unit)
    }

    fn set(&mut self, slot: usize, v: Value) {
        if slot < self.slots.len() {
            self.slots[slot] = v;
        }
    }

    fn push(&mut self, v: Value) {
        self.stack.push(v);
    }

    fn pop(&mut self) -> Value {
        self.stack.pop().unwrap_or(Value::Unit)
    }

    /// The value on top of the stack, left in place.
    fn peek_value(&self) -> Value {
        self.stack.last().cloned().unwrap_or(Value::Unit)
    }
}

struct Vm<'a> {
    code: Vec<Code>,
    /// The declarations, needed to build structs and read fields by name.
    decls: Vec<crate::ir::StructDef>,
    enums: Vec<crate::ir::EnumDef>,
    out: &'a mut dyn Write,
    depth: usize,
}

impl<'a> Vm<'a> {
    fn new(out: &'a mut dyn Write, program: &crate::bytecode::Program) -> Self {
        Vm {
            code: program.code.clone(),
            decls: program.structs.clone(),
            enums: program.enums.clone(),
            out,
            depth: 0,
        }
    }

    fn find(&self, name: &str) -> Option<Code> {
        self.code.iter().find(|c| c.name == name).cloned()
    }

    fn call(&mut self, name: &str, args: Vec<Value>, pos: Pos) -> Result<Value, RuntimeError> {
        let code = match self.find(name) {
            Some(c) => c,
            None => {
                return Err(RuntimeError::UnknownFunction {
                    name: name.to_string(),
                    pos,
                })
            }
        };

        self.depth += 1;
        if self.depth > MAX_DEPTH {
            self.depth -= 1;
            return Err(RuntimeError::BadControlFlow {
                pos,
                detail: "calls nested too deeply; the program may recurse forever".into(),
            });
        }

        let mut frame = Frame::new(code.frame_size);
        for (i, value) in args.into_iter().enumerate() {
            frame.set(i, value);
        }

        let result = self.execute(&code, &mut frame);
        self.depth -= 1;
        result
    }

    /// Runs one function to completion and returns the value on top of its
    /// stack.
    fn execute(&mut self, code: &Code, frame: &mut Frame) -> Result<Value, RuntimeError> {
        loop {
            if frame.ip >= code.instrs.len() {
                return Ok(frame.pop());
            }
            let instr = code.instrs[frame.ip].clone();
            frame.ip += 1;
            let pos = instr.pos;

            match &instr.op {
                Op::Const(c) => frame.push(const_value(c)),
                Op::Load(s) => {
                    let v = frame.get(*s);
                    frame.push(v);
                }
                Op::Store(s) => {
                    let v = frame.pop();
                    frame.set(*s, v);
                }

                Op::Binary(op) => {
                    let r = frame.pop();
                    let l = frame.pop();
                    frame.push(binary(*op, l, r, pos)?);
                }
                Op::Unary { neg } => {
                    let v = frame.pop();
                    frame.push(unary(*neg, v, pos)?);
                }

                Op::ListLen(n) => frame.pending_len = *n,
                Op::BuildList { tuple } => {
                    // The count comes from the ListLen the compiler emitted
                    // just before, so the VM does not walk anything to learn it.
                    let n = frame.pending_len;
                    let mut items = Vec::with_capacity(n);
                    for _ in 0..n {
                        items.push(frame.pop());
                    }
                    items.reverse();
                    frame.push(if *tuple {
                        Value::Tuple(items)
                    } else {
                        Value::list(items)
                    });
                }
                Op::Repeat => {
                    let n = frame.pop();
                    let v = frame.pop();
                    frame.push(Value::list(repeat(&v, &n, pos)?));
                }

                Op::Index => {
                    let i = frame.pop();
                    let b = frame.pop();
                    frame.push(index_into(&b, &i, pos)?);
                }
                // Reads a payload of the subject that MatchTest kept, without
                // needing an index on the stack.
                Op::Dup => {
                    let v = frame.peek_value();
                    frame.push(v);
                }
                Op::PayloadIndex(i) => {
                    let subject = frame.pop();
                    frame.push(payload(&subject, *i, pos)?);
                }
                Op::IndexStore => {
                    let v = frame.pop();
                    let i = frame.pop();
                    let slot = frame.pop();
                    store_into(&slot, &i, v.clone(), pos)?;
                    frame.push(v);
                }

                Op::Cast(ty) => {
                    let v = frame.pop();
                    frame.push(cast(&v, ty.name(), pos)?);
                }

                Op::Jump(t) => frame.ip = *t,
                Op::JumpIfFalse(t) => {
                    let c = frame.pop();
                    if !condition(&c, pos)? {
                        frame.ip = *t;
                    }
                }
                Op::BlockValue => {}
                Op::Pop => {
                    frame.pop();
                }

                Op::Break => {
                    if frame.loops == 0 {
                        return Err(RuntimeError::BadControlFlow {
                            pos,
                            detail: "`break` outside a loop".into(),
                        });
                    }
                    // The jump after this instruction leaves the loop, so no
                    // EndLoop will clear the flag. It is cleared here instead,
                    // or the next loop to reach an EndLoop sees a stale Break
                    // and leaves early.
                    frame.flow = Flow::Break;
                    // The jump that follows leaves the loop without reaching
                    // EndLoop, so the loop depth and the stack depth it opened
                    // are unwound here.
                    frame.loops = frame.loops.saturating_sub(1);
                    frame.loop_bases.pop();
                }
                Op::Continue => {
                    if frame.loops == 0 {
                        return Err(RuntimeError::BadControlFlow {
                            pos,
                            detail: "`continue` outside a loop".into(),
                        });
                    }
                    frame.flow = Flow::Continue;
                }

                Op::Return => return Ok(frame.pop()),

                Op::BuildPattern(p) => frame.push(pattern_value(p.clone())),
                Op::MatchTest { on_fail, keep } => {
                    let pattern = frame.pop();
                    let subject = frame.pop();
                    if pattern_matches(&pattern, &subject) {
                        // A pattern that binds names leaves the subject behind,
                        // so the following payload reads have something to index.
                        if *keep {
                            frame.push(subject);
                        }
                    } else {
                        frame.ip = *on_fail;
                    }
                }
                Op::MatchNone => {
                    return Err(RuntimeError::BadControlFlow {
                        pos,
                        detail: "no `match` arm matched, and there is no `_` arm".into(),
                    })
                }

                Op::Call { target, arity } => {
                    let mut args = Vec::with_capacity(*arity as usize);
                    for _ in 0..*arity {
                        args.push(frame.pop());
                    }
                    args.reverse();
                    frame.push(self.call_target(target, args, pos)?);
                }
                Op::FieldRead(name) => {
                    let b = frame.pop();
                    frame.push(read_field(&b, name, pos)?);
                }

                Op::Len => {
                    let v = frame.pop();
                    let n = match &v {
                        Value::List(cell) => cell.lock().expect("a list is not poisoned").len(),
                        Value::Tuple(items) => items.len(),
                        Value::Str(text) => text.chars().count(),
                        other => {
                            return Err(RuntimeError::BadOperands {
                                op: "len".to_string(),
                                lhs: other.type_name().to_string(),
                                rhs: String::new(),
                                pos,
                            })
                        }
                    };
                    frame.push(Value::Int(n as i64));
                }
                Op::LoopEnter => {
                    frame.loops += 1;
                    // The stack depth the body starts from. A back jump does
                    // not go through LoopEnter, so this runs once per entry
                    // rather than once per iteration. It used to run per
                    // iteration, which grew the stack without bound.
                    // Recorded before the condition pushes anything, so this is
                    // the depth a loop body starts from.
                    if frame.loop_bases.len() < frame.loops {
                        frame.loop_bases.push(frame.stack.len());
                    }
                }
                Op::LoopExit => {
                    frame.loops = frame.loops.saturating_sub(1);
                    if frame.loop_bases.len() > frame.loops {
                        frame.loop_bases.pop();
                    }
                }
                Op::EndLoop { on_break } => {
                    // A continue lands here and the flag decides whether the
                    // loop repeats. A break never reaches this instruction, so
                    // the flag is cleared where the break is raised.
                    // A loop body ends here. A flag set inside it decides
                    // whether the loop stops or repeats, and is cleared either
                    // way, which is what makes a nested loop work: the inner
                    // loop consumes the flag before the outer one sees it.
                    // EndLoop is the loop boundary, so the loop is left here.
                    // A break already unwound itself on the way out, because it
                    // jumps rather than falling through to this instruction.
                    if frame.flow != Flow::Break {
                        frame.loops = frame.loops.saturating_sub(1);
                        frame.loop_bases.pop();
                    }
                    match frame.flow {
                        Flow::Break => {
                            frame.flow = Flow::Normal;
                            frame.ip = *on_break;
                        }
                        Flow::Continue => frame.flow = Flow::Normal,
                        Flow::Normal => {}
                    }
                    // Restore the stack depth the body started from. The body
                    // leaves a different number of values depending on whether
                    // it fell through, continued or broke, so the depth is
                    // taken from the loop entry rather than guessed.
                    if let Some(base) = frame.loop_bases.last().copied() {
                        frame.stack.truncate(base);
                    }
                }
            }
        }
    }

    fn call_target(
        &mut self,
        target: &Target,
        args: Vec<Value>,
        pos: Pos,
    ) -> Result<Value, RuntimeError> {
        match target {
            Target::Function(name) => self.call(name, args, pos),
            Target::Struct(name) => {
                // A struct is built from its fields in declaration order, and
                // the compiler put them on the stack in that order.
                let names = self.struct_fields(name);
                if names.len() != args.len() {
                    return Err(RuntimeError::BadArity {
                        name: name.clone(),
                        want: names.len(),
                        got: args.len(),
                        pos,
                    });
                }
                Ok(Value::Struct {
                    name: name.clone(),
                    fields: names.into_iter().zip(args).collect(),
                })
            }
            Target::Variant { ty, variant } => {
                if !self.variant_declared(ty, variant) {
                    return Err(RuntimeError::UnknownFunction {
                        name: format!("{}.{}", ty, variant),
                        pos,
                    });
                }
                Ok(Value::Variant {
                    ty: ty.clone(),
                    variant: variant.clone(),
                    args,
                })
            }
            Target::Field(_) => {
                // A field read is not a call. The compiler emits FieldRead for
                // it, so reaching here means the bytecode was built wrong.
                Err(RuntimeError::BadControlFlow {
                    pos,
                    detail: "a field read reached the call path".into(),
                })
            }
            Target::Builtin(name) => self.builtin(name, args, pos),
        }
    }

    fn struct_fields(&self, name: &str) -> Vec<String> {
        self.decls
            .iter()
            .find(|d| d.name == name)
            .map(|d| d.fields.iter().map(|f| f.name.clone()).collect())
            .unwrap_or_default()
    }

    fn variant_declared(&self, ty: &str, variant: &str) -> bool {
        self.enums
            .iter()
            .find(|e| e.name == ty)
            .map(|e| e.variants.iter().any(|v| v.name == variant))
            .unwrap_or(false)
    }

    fn builtin(&mut self, name: &str, args: Vec<Value>, pos: Pos) -> Result<Value, RuntimeError> {
        macro_rules! arity {
            ($want:expr) => {{
                if args.len() != $want {
                    return Err(RuntimeError::BadArity {
                        name: name.to_string(),
                        want: $want,
                        got: args.len(),
                        pos,
                    });
                }
            }};
        }

        match name {
            "print" | "println" => {
                for v in &args {
                    write!(self.out, "{}", crate::interp::display(v))
                        .map_err(|e| io_error(e, pos))?;
                }
                if name == "println" {
                    writeln!(self.out).map_err(|e| io_error(e, pos))?;
                }
                Ok(Value::Unit)
            }
            "int_to_string" => {
                arity!(1);
                let n = as_int(&args[0], pos)?;
                Ok(Value::Str(n.to_string()))
            }
            "float_to_string" => {
                arity!(1);
                match &args[0] {
                    Value::Float(f) => Ok(Value::Str(format!("{:.6}", f))),
                    other => Err(mismatch("float_to_string", other, pos)),
                }
            }
            "string_to_int" => {
                arity!(1);
                match &args[0] {
                    Value::Str(s) => match s.trim().parse::<i64>() {
                        Ok(n) => Ok(Value::Int(n)),
                        Err(_) => Err(RuntimeError::BadOperands {
                            op: "string_to_int".to_string(),
                            lhs: "a Str that is not an integer".to_string(),
                            rhs: String::new(),
                            pos,
                        }),
                    },
                    other => Err(mismatch("string_to_int", other, pos)),
                }
            }
            "sqrt" => {
                arity!(1);
                match &args[0] {
                    Value::Float(f) => Ok(Value::Float(f.sqrt())),
                    Value::Int(n) => Ok(Value::Float((*n as f64).sqrt())),
                    other => Err(mismatch("sqrt", other, pos)),
                }
            }
            "abs" => {
                arity!(1);
                match &args[0] {
                    Value::Int(n) => Ok(Value::Int(n.abs())),
                    Value::Float(f) => Ok(Value::Float(f.abs())),
                    other => Err(mismatch("abs", other, pos)),
                }
            }
            "min" | "max" => {
                arity!(2);
                match (&args[0], &args[1]) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Int(if name == "min" {
                        (*x).min(*y)
                    } else {
                        (*x).max(*y)
                    })),
                    (Value::Float(x), Value::Float(y)) => Ok(Value::Float(if name == "min" {
                        x.min(*y)
                    } else {
                        x.max(*y)
                    })),
                    _ => Err(RuntimeError::BadOperands {
                        op: if name == "min" { "min" } else { "max" }.to_string(),
                        lhs: args[0].type_name().to_string(),
                        rhs: args[1].type_name().to_string(),
                        pos,
                    }),
                }
            }
            "now_ns" => {
                arity!(0);
                let d = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|_| RuntimeError::BadControlFlow {
                        pos,
                        detail: "the system clock is before the unix epoch".into(),
                    })?;
                Ok(Value::Int(d.as_nanos() as i64))
            }
            other => Err(RuntimeError::UnknownFunction {
                name: other.to_string(),
                pos,
            }),
        }
    }
}

// --- operations ------------------------------------------------------------

/// A compiled pattern as a runtime value, so a pattern can be pushed and tested
/// with the same machinery as anything else.
fn pattern_value(p: crate::bytecode::Pattern) -> Value {
    use crate::bytecode::Pattern;
    match p {
        Pattern::Wildcard => Value::Variant {
            ty: "#pattern".to_string(),
            variant: "wildcard".to_string(),
            args: Vec::new(),
        },
        Pattern::Literal(c) => Value::Variant {
            ty: "#pattern".to_string(),
            variant: "literal".to_string(),
            args: vec![const_value(&c)],
        },
        Pattern::Tuple(items) => Value::Variant {
            ty: "#pattern".to_string(),
            variant: "tuple".to_string(),
            args: items.into_iter().map(pattern_value).collect(),
        },
        Pattern::Variant { ty, variant } => Value::Variant {
            ty: "#pattern".to_string(),
            variant: "variant".to_string(),
            args: vec![Value::Str(ty), Value::Str(variant)],
        },
    }
}

/// Whether a value matches a compiled pattern.
fn pattern_matches(pattern: &Value, subject: &Value) -> bool {
    let (kind, args) = match pattern {
        Value::Variant { variant, args, .. } => (variant.as_str(), args),
        _ => return false,
    };
    match kind {
        "wildcard" => true,
        "literal" => args.first() == Some(subject),
        "tuple" => match subject {
            Value::List(items) => {
                let items = items.lock().expect("a list is not poisoned");
                items.len() == args.len()
                    && args
                        .iter()
                        .zip(items.iter())
                        .all(|(p, v)| pattern_matches(p, v))
            }
            Value::Tuple(items) => {
                items.len() == args.len()
                    && args
                        .iter()
                        .zip(items.iter())
                        .all(|(p, v)| pattern_matches(p, v))
            }
            _ => false,
        },
        "variant" => {
            let want_ty = args.first().and_then(|v| match v {
                Value::Str(s) => Some(s.as_str()),
                _ => None,
            });
            let want_variant = args.get(1).and_then(|v| match v {
                Value::Str(s) => Some(s.as_str()),
                _ => None,
            });
            match subject {
                Value::Variant { ty, variant, .. } => {
                    Some(ty.as_str()) == want_ty && Some(variant.as_str()) == want_variant
                }
                _ => false,
            }
        }
        _ => false,
    }
}

fn io_error(e: std::io::Error, pos: Pos) -> RuntimeError {
    RuntimeError::BadControlFlow {
        pos,
        detail: format!("could not write output: {}", e),
    }
}

fn const_value(c: &crate::ir::Const) -> Value {
    match c {
        crate::ir::Const::Int(v) => Value::Int(*v),
        crate::ir::Const::Float(v) => Value::Float(*v),
        crate::ir::Const::Str(s) => Value::Str(s.clone()),
        crate::ir::Const::Char(c) => Value::Char(*c),
        crate::ir::Const::Bool(b) => Value::Bool(*b),
    }
}

fn repeat(v: &Value, n: &Value, pos: Pos) -> Result<Vec<Value>, RuntimeError> {
    let n = match n {
        Value::Int(n) => *n,
        other => {
            return Err(RuntimeError::BadOperands {
                op: "a list repeat count".to_string(),
                lhs: "an Int".to_string(),
                rhs: other.type_name().to_string(),
                pos,
            })
        }
    };
    if n < 0 {
        return Err(RuntimeError::BadIndex {
            pos,
            detail: format!("cannot build a list of {} elements", n),
        });
    }
    Ok(vec![v.clone(); n as usize])
}

fn condition(v: &Value, pos: Pos) -> Result<bool, RuntimeError> {
    match v {
        Value::Bool(b) => Ok(*b),
        other => Err(RuntimeError::BadOperands {
            op: "a Bool in a condition".to_string(),
            lhs: other.type_name().to_string(),
            rhs: String::new(),
            pos,
        }),
    }
}

fn as_int(v: &Value, pos: Pos) -> Result<i64, RuntimeError> {
    match v {
        Value::Int(n) => Ok(*n),
        other => Err(RuntimeError::BadOperands {
            op: "an Int".to_string(),
            lhs: "an Int".to_string(),
            rhs: other.type_name().to_string(),
            pos,
        }),
    }
}

fn mismatch(op: &str, found: &Value, pos: Pos) -> RuntimeError {
    RuntimeError::BadOperands {
        op: op.to_string(),
        lhs: found.type_name().to_string(),
        rhs: String::new(),
        pos,
    }
}

fn unary(neg: bool, v: Value, pos: Pos) -> Result<Value, RuntimeError> {
    match (neg, v) {
        (false, Value::Bool(b)) => Ok(Value::Bool(!b)),
        (false, other) => Err(mismatch("!", &other, pos)),
        (true, Value::Int(n)) => Ok(Value::Int(-n)),
        (true, Value::Float(f)) => Ok(Value::Float(-f)),
        (true, other) => Err(mismatch("-", &other, pos)),
    }
}

fn cast(v: &Value, to: &str, pos: Pos) -> Result<Value, RuntimeError> {
    Ok(match (to, v) {
        ("Float", Value::Int(n)) => Value::Float(*n as f64),
        ("Int", Value::Float(f)) => Value::Int(*f as i64),
        ("Char", Value::Int(n)) => match u32::try_from(*n).ok().and_then(char::from_u32) {
            Some(c) => Value::Char(c),
            None => {
                return Err(RuntimeError::BadOperands {
                    op: "as Char".to_string(),
                    lhs: format!("{} is not a Unicode scalar value", n),
                    rhs: String::new(),
                    pos,
                })
            }
        },
        ("Int", Value::Char(c)) => Value::Int(*c as i64),
        _ => v.clone(),
    })
}

fn index_into(base: &Value, index: &Value, pos: Pos) -> Result<Value, RuntimeError> {
    let i = match index {
        Value::Int(n) => *n,
        other => {
            return Err(RuntimeError::BadIndex {
                pos,
                detail: format!("an index must be an Int, found {}", other.type_name()),
            })
        }
    };
    if i < 0 {
        return Err(RuntimeError::BadIndex {
            pos,
            detail: format!("index {} is negative", i),
        });
    }
    let i = i as usize;
    match base {
        Value::List(items) => {
            let items = items.lock().expect("a list is not poisoned");
            items.get(i).cloned().ok_or(RuntimeError::BadIndex {
                pos,
                detail: format!("index {} is past the end of {} values", i, items.len()),
            })
        }
        // A variant's payloads are read by position, which is what a pattern
        // binding needs. The checker has already checked the arity.
        Value::Variant { ty, args, .. } => args.get(i).cloned().ok_or(RuntimeError::BadIndex {
            pos,
            detail: format!(
                "index {} is past the end of {} payloads on {}",
                i,
                args.len(),
                ty
            ),
        }),
        Value::Tuple(items) => items.get(i).cloned().ok_or(RuntimeError::BadIndex {
            pos,
            detail: format!("index {} is past the end of {} values", i, items.len()),
        }),
        Value::Str(s) => {
            let chars: Vec<char> = s.chars().collect();
            chars
                .get(i)
                .copied()
                .map(Value::Char)
                .ok_or(RuntimeError::BadIndex {
                    pos,
                    detail: format!("index {} is past the end of {} characters", i, chars.len()),
                })
        }
        other => Err(RuntimeError::BadIndex {
            pos,
            detail: format!("{} cannot be indexed", other.type_name()),
        }),
    }
}

/// Reads one payload of a variant by position.
fn payload(subject: &Value, i: usize, pos: Pos) -> Result<Value, RuntimeError> {
    match subject {
        Value::Variant { ty, args, .. } => args.get(i).cloned().ok_or(RuntimeError::BadIndex {
            pos,
            detail: format!(
                "index {} is past the end of {} payloads on {}",
                i,
                args.len(),
                ty
            ),
        }),
        other => Err(RuntimeError::BadIndex {
            pos,
            detail: format!("{} has no payloads", other.type_name()),
        }),
    }
}

fn store_into(slot: &Value, index: &Value, v: Value, pos: Pos) -> Result<(), RuntimeError> {
    let i = match index {
        Value::Int(n) => *n,
        other => {
            return Err(RuntimeError::BadIndex {
                pos,
                detail: format!("an index must be an Int, found {}", other.type_name()),
            })
        }
    };
    if i < 0 {
        return Err(RuntimeError::BadIndex {
            pos,
            detail: format!("index {} is negative", i),
        });
    }
    let i = i as usize;
    match slot {
        Value::List(cell) => {
            let mut items = cell.lock().expect("a list is not poisoned");
            if i >= items.len() {
                return Err(RuntimeError::BadIndex {
                    pos,
                    detail: format!("index {} is past the end of {} values", i, items.len()),
                });
            }
            items[i] = v;
            Ok(())
        }
        other => Err(RuntimeError::BadIndex {
            pos,
            detail: format!("{} cannot be indexed", other.type_name()),
        }),
    }
}

fn read_field(base: &Value, name: &str, pos: Pos) -> Result<Value, RuntimeError> {
    if let Some(i) = name.parse::<usize>().ok() {
        let found = match base {
            Value::Tuple(items) => items.get(i).cloned(),
            Value::Variant { args, .. } => args.get(i).cloned(),
            _ => None,
        };
        return match found {
            Some(v) => Ok(v),
            None => Err(RuntimeError::UnknownField {
                name: name.to_string(),
                pos,
            }),
        };
    }
    match base {
        Value::Struct { fields, .. } => fields
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
            .ok_or_else(|| RuntimeError::UnknownField {
                name: name.to_string(),
                pos,
            }),
        other => Err(RuntimeError::NotAStruct {
            pos,
            found: other.type_name().to_string(),
        }),
    }
}

fn binary(op: ast::BinOp, l: Value, r: Value, pos: Pos) -> Result<Value, RuntimeError> {
    match op {
        ast::BinOp::Eq => Ok(Value::Bool(l == r)),
        ast::BinOp::Ne => Ok(Value::Bool(l != r)),
        ast::BinOp::And => match (&l, &r) {
            (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a && *b)),
            _ => Err(mismatch2("&&", &l, &r, pos)),
        },
        ast::BinOp::Or => match (&l, &r) {
            (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a || *b)),
            _ => Err(mismatch2("||", &l, &r, pos)),
        },
        ast::BinOp::Lt | ast::BinOp::Le | ast::BinOp::Gt | ast::BinOp::Ge => {
            let ord = match (&l, &r) {
                (Value::Int(a), Value::Int(b)) => a.partial_cmp(b),
                (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
                (Value::Str(a), Value::Str(b)) => Some(a.cmp(b)),
                (Value::Char(a), Value::Char(b)) => Some(a.cmp(b)),
                _ => return Err(mismatch2(op.spelling(), &l, &r, pos)),
            };
            let ord = match ord {
                Some(o) => o,
                None => return Err(mismatch2(op.spelling(), &l, &r, pos)),
            };
            use std::cmp::Ordering::*;
            Ok(Value::Bool(match op {
                ast::BinOp::Lt => ord == Less,
                ast::BinOp::Le => ord != Greater,
                ast::BinOp::Gt => ord == Greater,
                _ => ord != Less,
            }))
        }
        ast::BinOp::Add => match (&l, &r) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_add(*b))),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
            (Value::Str(a), Value::Str(b)) => Ok(Value::Str(format!("{}{}", a, b))),
            _ => Err(mismatch2(op.spelling(), &l, &r, pos)),
        },
        ast::BinOp::Sub => match (&l, &r) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_sub(*b))),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
            _ => Err(mismatch2(op.spelling(), &l, &r, pos)),
        },
        ast::BinOp::Mul => match (&l, &r) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_mul(*b))),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
            _ => Err(mismatch2(op.spelling(), &l, &r, pos)),
        },
        ast::BinOp::Div => match (&l, &r) {
            (Value::Int(a), Value::Int(b)) => {
                if *b == 0 {
                    Err(RuntimeError::DivideByZero { pos })
                } else {
                    Ok(Value::Int(a.wrapping_div(*b)))
                }
            }
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a / b)),
            _ => Err(mismatch2(op.spelling(), &l, &r, pos)),
        },
        ast::BinOp::Rem => match (&l, &r) {
            (Value::Int(a), Value::Int(b)) => {
                if *b == 0 {
                    Err(RuntimeError::DivideByZero { pos })
                } else {
                    Ok(Value::Int(a.wrapping_rem(*b)))
                }
            }
            _ => Err(mismatch2(op.spelling(), &l, &r, pos)),
        },
    }
}

fn mismatch2(op: &str, l: &Value, r: &Value, pos: Pos) -> RuntimeError {
    RuntimeError::BadOperands {
        op: op.to_string(),
        lhs: l.type_name().to_string(),
        rhs: r.type_name().to_string(),
        pos,
    }
}
