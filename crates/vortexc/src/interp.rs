//! The tree interpreter.
//!
//! Executes the lowered form from [`crate::ir`]. Stage 4 adds a bytecode VM over
//! the same lowered form, which is why lowering happens once, in stage 2, as
//! `SPEC.md` section 10.1 asks.
//!
//! This stage does not check types. The checks that can be sound without a type
//! system, such as dividing by zero or indexing past the end of a collection,
//! are done here and reported with the position the lowering carried through.
//! Everything else is stage 3.

use std::io::Write;
use std::sync::Arc;

use crate::ir;
use crate::span::Pos;
use crate::value::{EvalResult, RuntimeError, Value};

/// How deeply calls may nest before the interpreter gives up, so a runaway
/// recursion reports an error instead of exhausting the machine stack.
const MAX_DEPTH: usize = 800;

/// The stack the interpreter runs on, in bytes.
///
/// A tree walk uses a lot of native stack per Vortex call, and the default
/// thread stack is too small for `MAX_DEPTH` frames. Running on a thread with
/// this much stack is what makes the depth guard above fire first and report an
/// error, instead of the process dying with a stack overflow.
const STACK_BYTES: usize = 64 * 1024 * 1024;

/// How a block finished.
enum Flow {
    /// Fell off the end, yielding the trailing expression or Unit.
    Value(Value),
    /// A `return` left the block, with the returned value.
    Return(Value),
    /// A `break` left the block.
    Break,
    /// A `continue` left the block.
    Continue,
}

/// The result of evaluating an expression, which may have left a construct that
/// was not its own. `Flow::Value` is the ordinary case.
enum Eval {
    Value(Value),
    Return(Value),
    Break,
    Continue,
}

impl Eval {
    fn into_flow(self) -> Flow {
        match self {
            Eval::Value(v) => Flow::Value(v),
            Eval::Return(v) => Flow::Return(v),
            Eval::Break => Flow::Break,
            Eval::Continue => Flow::Continue,
        }
    }

    fn value(self) -> Value {
        match self {
            Eval::Value(v) | Eval::Return(v) => v,
            _ => Value::Unit,
        }
    }
}

/// Runs a lowered program, writing whatever it prints to `out`.
///
/// The program must declare `main`. The value `main` returns is returned here,
/// which is how a test checks a result without printing it.
///
/// The interpreter runs on its own thread with a stack large enough for
/// `MAX_DEPTH` frames, so the depth guard reports an error instead of the
/// process dying with a stack overflow. The program is collected on that thread
/// and the output is then written to `out` on this one, which keeps the writer
/// from having to cross a thread boundary.
pub fn run(program: &ir::Program, out: &mut dyn Write) -> Result<Value, RuntimeError> {
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

/// Runs a lowered program and returns its value together with what it printed.
///
/// `ir::Program` is immutable and owned by the caller, and the thread is joined
/// before this returns, so a shared reference may be passed to it.
pub fn run_collecting_output(program: &ir::Program) -> Result<(Value, String), RuntimeError> {
    let program = Arc::new(program.clone());
    let handle = std::thread::Builder::new()
        .name("vortex-interp".to_string())
        .stack_size(STACK_BYTES)
        .spawn(move || {
            let mut buffer: Vec<u8> = Vec::new();
            let result = {
                let mut vm = Vm::new(&mut buffer);
                vm.load(&program);
                vm.call_function("main", Vec::new(), Pos::START)
            };
            (result, buffer)
        })
        .expect("the interpreter thread should start");

    match handle.join() {
        Ok((result, buffer)) => {
            let text = String::from_utf8(buffer).expect("output should be valid UTF-8");
            result.map(|v| (v, text))
        }
        Err(_) => Err(RuntimeError::BadControlFlow {
            pos: Pos::START,
            detail: "the interpreter thread panicked".into(),
        }),
    }
}

/// One call frame. Slots are laid out when the call is made, using the frame
/// size the lowering recorded, so a frame never grows while a program runs.
struct Frame {
    slots: Vec<Value>,
}

impl Frame {
    fn new(size: usize) -> Frame {
        Frame {
            slots: vec![Value::Unit; size],
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
}

struct Vm<'a> {
    functions: Vec<ir::Fn>,
    structs: Vec<ir::StructDef>,
    enums: Vec<ir::EnumDef>,
    out: &'a mut dyn Write,
    depth: usize,
}

impl<'a> Vm<'a> {
    fn new(out: &'a mut dyn Write) -> Self {
        Vm {
            functions: Vec::new(),
            structs: Vec::new(),
            enums: Vec::new(),
            out,
            depth: 0,
        }
    }

    fn load(&mut self, program: &ir::Program) {
        for item in &program.items {
            match item {
                ir::Item::Function(f) => self.functions.push(f.clone()),
                ir::Item::Struct(s) => self.structs.push(s.clone()),
                ir::Item::Enum(e) => self.enums.push(e.clone()),
            }
        }
    }

    fn call_function(&mut self, name: &str, args: Vec<Value>, pos: Pos) -> EvalResult {
        let f = match self.functions.iter().find(|f| f.name == name) {
            Some(f) => f.clone(),
            None => {
                return Err(RuntimeError::UnknownFunction {
                    name: name.to_string(),
                    pos,
                })
            }
        };

        if args.len() != f.params.len() {
            return Err(RuntimeError::BadArity {
                name: name.to_string(),
                want: f.params.len(),
                got: args.len(),
                pos,
            });
        }

        self.depth += 1;
        if self.depth > MAX_DEPTH {
            self.depth -= 1;
            return Err(RuntimeError::BadControlFlow {
                pos,
                detail: "calls nested too deeply; the program may recurse forever".into(),
            });
        }

        let mut frame = Frame::new(f.frame_size);
        for (p, value) in f.params.iter().zip(args) {
            frame.set(p.slot, value);
        }

        let result = self.exec_block(&f.body, &mut frame);
        self.depth -= 1;

        match result? {
            Flow::Value(v) | Flow::Return(v) => Ok(v),
            // A `break` or `continue` that escaped a function body has no loop
            // to belong to, so it is a run time error rather than a value.
            Flow::Break => Err(RuntimeError::BadControlFlow {
                pos,
                detail: "`break` outside a loop".into(),
            }),
            Flow::Continue => Err(RuntimeError::BadControlFlow {
                pos,
                detail: "`continue` outside a loop".into(),
            }),
        }
    }

    // --- blocks and statements ------------------------------------------

    fn exec_block(&mut self, b: &ir::Block, frame: &mut Frame) -> Result<Flow, RuntimeError> {
        for stmt in &b.stmts {
            let flow = self.exec_stmt(stmt, frame)?;
            // Anything that is not an ordinary value left this block.
            if !matches!(flow, Flow::Value(_)) {
                return Ok(flow);
            }
        }
        match &b.tail {
            Some(t) => Ok(self.eval(t, frame)?.into_flow()),
            None => Ok(Flow::Value(Value::Unit)),
        }
    }

    fn exec_stmt(&mut self, s: &ir::Stmt, frame: &mut Frame) -> Result<Flow, RuntimeError> {
        match s {
            ir::Stmt::Let { slot, init, .. } => {
                let v = self.eval(init, frame)?.value();
                frame.set(*slot, v);
                Ok(Flow::Value(Value::Unit))
            }
            ir::Stmt::Assign { slot, value, .. } => {
                let v = self.eval(value, frame)?.value();
                frame.set(*slot, v);
                Ok(Flow::Value(Value::Unit))
            }
            ir::Stmt::Return(e) => Ok(Flow::Return(self.eval(e, frame)?.value())),
            ir::Stmt::Nop { expr, .. } => {
                // The expression may contain a `return`, `break` or
                // `continue`, for instance an `if` used as a statement. Those
                // must not be discarded, or the statements after the `if`
                // would run.
                Ok(self.eval(expr, frame)?.into_flow())
            }
            ir::Stmt::Break => Ok(Flow::Break),
            ir::Stmt::Continue => Ok(Flow::Continue),

            ir::Stmt::While { cond, body, pos } => loop {
                let c = self.eval(cond, frame)?.value();
                if !condition(&c, *pos)? {
                    return Ok(Flow::Value(Value::Unit));
                }
                match self.exec_block(body, frame)? {
                    Flow::Break => return Ok(Flow::Value(Value::Unit)),
                    Flow::Value(_) | Flow::Continue => {}
                    Flow::Return(v) => return Ok(Flow::Return(v)),
                }
            },

            ir::Stmt::ForEach {
                var_slot,
                iterable,
                body,
                pos,
            } => {
                let it = self.eval(iterable, frame)?.value();
                let items: Vec<Value> = match it {
                    Value::Array(items) | Value::Tuple(items) => items,
                    Value::Str(text) => text.chars().map(Value::Char).collect(),
                    other => {
                        return Err(RuntimeError::BadControlFlow {
                            pos: *pos,
                            detail: format!(
                                "`for` can walk a list, a tuple or a string, not {}",
                                other.type_name()
                            ),
                        })
                    }
                };
                for item in items {
                    frame.set(*var_slot, item);
                    match self.exec_block(body, frame)? {
                        Flow::Break => return Ok(Flow::Value(Value::Unit)),
                        Flow::Value(_) | Flow::Continue => {}
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                    }
                }
                Ok(Flow::Value(Value::Unit))
            }

            ir::Stmt::For {
                var_slot,
                start,
                end,
                inclusive,
                body,
                pos,
            } => {
                let from = self.eval(start, frame)?.value();
                let to = self.eval(end, frame)?.value();
                let from = as_int(&from, *pos, "a `for` range start")?;
                let to = as_int(&to, *pos, "a `for` range end")?;
                let stop = if *inclusive { to + 1 } else { to };
                let mut i = from;
                while i < stop {
                    frame.set(*var_slot, Value::Int(i));
                    match self.exec_block(body, frame)? {
                        Flow::Break => return Ok(Flow::Value(Value::Unit)),
                        Flow::Value(_) | Flow::Continue => {}
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                    }
                    i += 1;
                }
                Ok(Flow::Value(Value::Unit))
            }
        }
    }

    // --- expressions -----------------------------------------------------

    /// Evaluates an expression. An expression may contain a `return`, `break`
    /// or `continue`, so the result says whether it left the construct around
    /// it.
    fn eval(&mut self, e: &ir::Expr, frame: &mut Frame) -> Result<Eval, RuntimeError> {
        Ok(match e {
            ir::Expr::Const { value, .. } => Eval::Value(const_value(value)),
            ir::Expr::Load { slot, .. } => Eval::Value(frame.get(*slot)),
            ir::Expr::Store { slot, value, .. } => {
                let v = self.eval(value, frame)?.value();
                frame.set(*slot, v.clone());
                Eval::Value(v)
            }
            ir::Expr::Unary { neg, operand, pos } => {
                let v = self.eval(operand, frame)?.value();
                Eval::Value(unary(*neg, v, *pos)?)
            }
            ir::Expr::Binary { op, lhs, rhs, pos } => {
                // Both sides are evaluated before the operation, so `&&` and
                // `||` do not short circuit in this stage. Stage 3 makes that
                // visible in the type rules.
                let l = self.eval(lhs, frame)?.value();
                let r = self.eval(rhs, frame)?.value();
                Eval::Value(binary(*op, l, r, *pos)?)
            }
            ir::Expr::List { items, tuple, .. } => {
                let mut values = Vec::with_capacity(items.len());
                for i in items {
                    values.push(self.eval(i, frame)?.value());
                }
                Eval::Value(if *tuple {
                    Value::Tuple(values)
                } else {
                    Value::Array(values)
                })
            }
            // `[value; count]` builds a list whose length is known only at run
            // time, which is what the benchmark workload needs.
            ir::Expr::Repeat { value, count, pos } => {
                let v = self.eval(value, frame)?.value();
                let n = self.eval(count, frame)?.value();
                let n = match &n {
                    Value::Int(n) => *n,
                    other => {
                        return Err(RuntimeError::BadOperands {
                            op: "a list repeat count".to_string(),
                            lhs: "an Int".to_string(),
                            rhs: other.type_name().to_string(),
                            pos: *pos,
                        })
                    }
                };
                if n < 0 {
                    return Err(RuntimeError::BadIndex {
                        pos: *pos,
                        detail: format!("cannot build a list of {} elements", n),
                    });
                }
                Eval::Value(Value::Array(vec![v; n as usize]))
            }

            // `a[i] = v` stores into a list element, then writes the changed
            // list back into the slot that holds it. Without the write back the
            // store would only change a copy.
            ir::Expr::IndexStore {
                slot,
                index,
                value,
                pos,
            } => {
                let mut list = frame.get(*slot);
                let at = self.eval(index, frame)?.value();
                let v = self.eval(value, frame)?.value();
                let at = match &at {
                    Value::Int(n) => *n,
                    other => {
                        return Err(RuntimeError::BadIndex {
                            pos: *pos,
                            detail: format!("an index must be an Int, found {}", other.type_name()),
                        })
                    }
                };
                if at < 0 {
                    return Err(RuntimeError::BadIndex {
                        pos: *pos,
                        detail: format!("index {} is negative", at),
                    });
                }
                let at = at as usize;
                let items = match &mut list {
                    Value::Array(items) => items,
                    other => {
                        return Err(RuntimeError::BadIndex {
                            pos: *pos,
                            detail: format!("{} cannot be indexed", other.type_name()),
                        })
                    }
                };
                if at >= items.len() {
                    return Err(RuntimeError::BadIndex {
                        pos: *pos,
                        detail: format!("index {} is past the end of {} values", at, items.len()),
                    });
                }
                items[at] = v.clone();
                // Write the changed list back, or the store only touched a copy.
                frame.set(*slot, list);
                Eval::Value(v)
            }

            ir::Expr::Index { base, index, pos } => {
                let b = self.eval(base, frame)?.value();
                let i = self.eval(index, frame)?.value();
                Eval::Value(index_into(b, i, *pos)?)
            }
            ir::Expr::Field { base, name, pos } => {
                let b = self.eval(base, frame)?.value();
                Eval::Value(read_field(b, name, *pos)?)
            }
            ir::Expr::Try { inner, .. } => self.eval(inner, frame)?,
            ir::Expr::BlockValue(b) => self.exec_block(b, frame)?.into_eval(),

            ir::Expr::If {
                cond,
                then,
                otherwise,
                pos,
            } => {
                let c = self.eval(cond, frame)?.value();
                if condition(&c, *pos)? {
                    self.exec_block(then, frame)?.into_eval()
                } else {
                    match otherwise {
                        None => Eval::Value(Value::Unit),
                        Some(other) => self.eval(other, frame)?,
                    }
                }
            }

            ir::Expr::Call {
                target,
                args,
                arg_slots,
                pos,
            } => {
                let mut values = Vec::with_capacity(args.len());
                for (i, a) in args.iter().enumerate() {
                    let v = self.eval(a, frame)?.value();
                    // A declared function reads its arguments from slots the
                    // lowering reserved in this frame. A builtin or a
                    // constructor has no frame of its own, so it takes the
                    // values directly.
                    if let Some(slot) = arg_slots.get(i) {
                        frame.set(*slot, v.clone());
                    }
                    values.push(v);
                }
                Eval::Value(self.call(target, values, *pos)?)
            }
            ir::Expr::Match {
                scrutinee,
                arms,
                pos,
            } => {
                let value = self.eval(scrutinee, frame)?.value();
                for arm in arms {
                    if self.match_pattern(&arm.pattern, &value, frame) {
                        return self.eval(&arm.body, frame);
                    }
                }
                Err(RuntimeError::BadControlFlow {
                    pos: *pos,
                    detail: "no `match` arm matched, and there is no `_` arm".into(),
                })?
            }
        })
    }

    fn match_pattern(&self, p: &ir::Pattern, value: &Value, frame: &mut Frame) -> bool {
        match p {
            ir::Pattern::Wildcard => true,
            ir::Pattern::Binding(b) => {
                frame.set(b.slot, value.clone());
                true
            }
            ir::Pattern::Literal(c) => &const_value(c) == value,
            ir::Pattern::Tuple(items) => match value {
                Value::Tuple(values) | Value::Array(values) => {
                    if values.len() != items.len() {
                        return false;
                    }
                    for (p, v) in items.iter().zip(values) {
                        if !self.match_pattern(p, v, frame) {
                            return false;
                        }
                    }
                    true
                }
                _ => false,
            },
            ir::Pattern::Variant {
                ty,
                variant,
                bindings,
            } => match value {
                Value::Variant {
                    ty: vty,
                    variant: vv,
                    args,
                } => {
                    if vty != ty || vv != variant || args.len() != bindings.len() {
                        return false;
                    }
                    for (b, v) in bindings.iter().zip(args) {
                        frame.set(b.slot, v.clone());
                    }
                    true
                }
                _ => false,
            },
        }
    }

    // --- calls -----------------------------------------------------------

    fn call(&mut self, target: &ir::CallTarget, args: Vec<Value>, pos: Pos) -> EvalResult {
        match target {
            ir::CallTarget::Function(name) => self.call_function(name, args, pos),

            ir::CallTarget::Struct(name) => {
                let fields = match self.structs.iter().find(|s| s.name == *name) {
                    Some(s) => s.fields.clone(),
                    None => {
                        return Err(RuntimeError::UnknownFunction {
                            name: name.clone(),
                            pos,
                        })
                    }
                };
                if args.len() != fields.len() {
                    return Err(RuntimeError::BadArity {
                        name: name.clone(),
                        want: fields.len(),
                        got: args.len(),
                        pos,
                    });
                }
                let values = fields.into_iter().map(|f| f.name).zip(args).collect();
                Ok(Value::Struct {
                    name: name.clone(),
                    fields: values,
                })
            }

            ir::CallTarget::Variant { ty, variant } => {
                let declared = self
                    .enums
                    .iter()
                    .find(|e| e.name == *ty)
                    .map(|e| e.variants.iter().any(|v| v.name == *variant))
                    .unwrap_or(false);
                if !declared {
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

            ir::CallTarget::Builtin(name) => self.builtin(name, args, pos),
        }
    }

    // --- builtins --------------------------------------------------------

    fn builtin(&mut self, name: &str, args: Vec<Value>, pos: Pos) -> EvalResult {
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
                    write!(self.out, "{}", display(v)).map_err(|e| io_error(e, pos))?;
                }
                if name == "println" {
                    writeln!(self.out).map_err(|e| io_error(e, pos))?;
                }
                Ok(Value::Unit)
            }

            "int_to_string" => {
                arity!(1);
                let n = as_int(&args[0], pos, "an Int")?;
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
                    _ => Err(mismatch2(
                        if name == "min" { "min" } else { "max" },
                        &args[0],
                        &args[1],
                        pos,
                    )),
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

impl Flow {
    /// Converts a finished block into an expression result. A `return`,
    /// `break` or `continue` keeps its meaning: an `if` used as an expression
    /// must not swallow a `return` written inside it, or the statements after
    /// the `if` would run.
    fn into_eval(self) -> Eval {
        match self {
            Flow::Value(v) => Eval::Value(v),
            Flow::Return(v) => Eval::Return(v),
            Flow::Break => Eval::Break,
            Flow::Continue => Eval::Continue,
        }
    }
}

// --- operations ------------------------------------------------------------

fn io_error(e: std::io::Error, pos: Pos) -> RuntimeError {
    RuntimeError::BadControlFlow {
        pos,
        detail: format!("could not write output: {}", e),
    }
}

fn const_value(c: &ir::Const) -> Value {
    match c {
        ir::Const::Int(v) => Value::Int(*v),
        ir::Const::Float(v) => Value::Float(*v),
        ir::Const::Str(s) => Value::Str(s.clone()),
        ir::Const::Char(c) => Value::Char(*c),
        ir::Const::Bool(b) => Value::Bool(*b),
    }
}

fn as_int(v: &Value, pos: Pos, expectation: &str) -> Result<i64, RuntimeError> {
    match v {
        Value::Int(n) => Ok(*n),
        other => Err(RuntimeError::BadOperands {
            op: "an Int".to_string(),
            lhs: expectation.to_string(),
            rhs: other.type_name().to_string(),
            pos,
        }),
    }
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

fn unary(neg: bool, v: Value, pos: Pos) -> EvalResult {
    match (neg, v) {
        (false, Value::Bool(b)) => Ok(Value::Bool(!b)),
        (false, other) => Err(mismatch("!", &other, pos)),
        (true, Value::Int(n)) => Ok(Value::Int(-n)),
        (true, Value::Float(f)) => Ok(Value::Float(-f)),
        (true, other) => Err(mismatch("-", &other, pos)),
    }
}

fn binary(op: crate::ast::BinOp, l: Value, r: Value, pos: Pos) -> EvalResult {
    use crate::ast::BinOp;
    use std::cmp::Ordering;

    match op {
        BinOp::Eq => Ok(Value::Bool(l == r)),
        BinOp::Ne => Ok(Value::Bool(l != r)),

        BinOp::And => match (&l, &r) {
            (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a && *b)),
            _ => Err(mismatch2("&&", &l, &r, pos)),
        },
        BinOp::Or => match (&l, &r) {
            (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a || *b)),
            _ => Err(mismatch2("||", &l, &r, pos)),
        },

        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
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
            Ok(Value::Bool(match op {
                BinOp::Lt => ord == Ordering::Less,
                BinOp::Le => ord != Ordering::Greater,
                BinOp::Gt => ord == Ordering::Greater,
                _ => ord != Ordering::Less,
            }))
        }

        BinOp::Add => match (&l, &r) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_add(*b))),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
            (Value::Str(a), Value::Str(b)) => Ok(Value::Str(format!("{}{}", a, b))),
            _ => Err(mismatch2(op.spelling(), &l, &r, pos)),
        },
        BinOp::Sub => match (&l, &r) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_sub(*b))),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
            _ => Err(mismatch2(op.spelling(), &l, &r, pos)),
        },
        BinOp::Mul => match (&l, &r) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_mul(*b))),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
            _ => Err(mismatch2(op.spelling(), &l, &r, pos)),
        },
        BinOp::Div => match (&l, &r) {
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
        BinOp::Rem => match (&l, &r) {
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

fn mismatch(op: &str, found: &Value, pos: Pos) -> RuntimeError {
    RuntimeError::BadOperands {
        op: op.to_string(),
        lhs: found.type_name().to_string(),
        rhs: String::new(),
        pos,
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

fn index_into(base: Value, index: Value, pos: Pos) -> EvalResult {
    let i = match &index {
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
    match &base {
        Value::Array(items) | Value::Tuple(items) => {
            items.get(i).cloned().ok_or(RuntimeError::BadIndex {
                pos,
                detail: format!("index {} is past the end of {} values", i, items.len()),
            })
        }
        Value::Str(s) => {
            let chars: Vec<char> = s.chars().collect();
            chars
                .get(i)
                .map(|c| Value::Char(*c))
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

fn read_field(base: Value, name: &str, pos: Pos) -> EvalResult {
    // A tuple or a variant payload is read by position, as `t.0` and `s.1`.
    // The parser accepts a number after a dot for exactly this, and the checker
    // gives the element's type, so the interpreter serves the element here.
    if let Some(index) = name.parse::<usize>().ok() {
        let found = match &base {
            Value::Tuple(items) => items.get(index).cloned(),
            Value::Variant { args, .. } => args.get(index).cloned(),
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

    match &base {
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

/// How a value is printed. The float format matches the C and Rust baselines,
/// so the same value prints the same text in all three.
pub fn display(v: &Value) -> String {
    match v {
        Value::Int(n) => n.to_string(),
        Value::Float(f) => format!("{:.6}", f),
        Value::Bool(b) => b.to_string(),
        Value::Str(s) => s.clone(),
        Value::Char(c) => c.to_string(),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(display).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Tuple(items) => {
            let parts: Vec<String> = items.iter().map(display).collect();
            format!("({})", parts.join(", "))
        }
        Value::Struct { name, fields } => {
            let parts: Vec<String> = fields
                .iter()
                .map(|(n, v)| format!("{}: {}", n, display(v)))
                .collect();
            format!("{}{{{}}}", name, parts.join(", "))
        }
        Value::Variant { ty, variant, args } => {
            if args.is_empty() {
                format!("{}.{}", ty, variant)
            } else {
                let parts: Vec<String> = args.iter().map(display).collect();
                format!("{}.{}({})", ty, variant, parts.join(", "))
            }
        }
        Value::Func(name) => format!("fn {}", name),
        Value::Unit => "()".to_string(),
    }
}
