//! Bytecode: the instruction set and the compiler that produces it.
//!
//! `SPEC.md` section 10.1 asks for the frontend to lower once so that stage 4
//! adds a consumer rather than rewriting the frontend. This is that second
//! consumer. It reads the same [`crate::ir::Program`] the tree interpreter
//! reads and produces a flat instruction list per function.
//!
//! Two decisions are worth stating, because they shape everything else.
//!
//! **Break and continue are flags, not jumps.** Compiling them to a jump means
//! patching every unresolved jump when a loop closes, which is fiddly and easy
//! to get wrong in a nested case. The frame carries a small flow flag instead,
//! and a loop reads it. That costs one comparison per iteration and removes a
//! whole class of compiler bugs.
//!
//! **Every opcode carries its position.** A run time error therefore names the
//! line and column without the VM needing a side table, and a diagnostic from
//! the VM is shaped exactly like one from the tree interpreter.

use crate::ast;
use crate::ir;
use crate::span::Pos;

/// An index into a call frame.
pub type Slot = usize;

/// One instruction with the position it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Instr {
    pub op: Op,
    pub pos: Pos,
}

/// The instruction set.
///
/// Every opcode has a direct counterpart in the tree walk, so a disagreement
/// between the two executors shows up as a wrong answer rather than as a
/// subtly different program.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// Push a constant.
    Const(ir::Const),
    /// Read a slot.
    Load(Slot),
    /// Write a slot.
    Store(Slot),
    /// An arithmetic or comparison operator.
    Binary(ast::BinOp),
    /// Negate, or `!`.
    Unary { neg: bool },
    /// Call a function, a builtin, a constructor or a variant.
    Call { target: Target, arity: u32 },
    /// Read a list element. The base and the index are below it.
    Index,
    /// Store into a list element. The slot, the index and the value are below.
    IndexStore,
    /// Build a list from the values above it. `tuple` records which.
    BuildList { tuple: bool },
    /// Build `[value; count]`. The value and the count are below.
    Repeat,
    /// Convert the value below.
    Cast(ast::TypeExpr),
    /// Jump when the value below is false, consuming it.
    JumpIfFalse(usize),
    /// An unconditional jump.
    Jump(usize),
    /// Evaluate a block and keep its value.
    BlockValue,
    /// Discard the value on top of the stack.
    Pop,
    /// Leave a loop early.
    Break,
    /// Skip to the next iteration.
    Continue,
    /// Return the value on top of the stack.
    Return,
    /// Match the value below against arm `arm` of the enclosing match.
    MatchTest { arm: u32 },
    /// No arm of a match matched.
    MatchNone,
}

/// What a call refers to.
///
/// The named forms come straight from the lowering pass. `Field` is added here
/// because a field read is not a call in the tree but the VM resolves it by
/// name rather than by index, which keeps the compiler free of the struct
/// declarations.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Function(String),
    Builtin(String),
    Struct(String),
    Variant {
        ty: String,
        variant: String,
    },
    /// Read one named field of the value below.
    Field(String),
}

impl Target {
    /// The name a disassembly prints for a call.
    pub fn name(&self) -> String {
        match self {
            Target::Function(n) | Target::Builtin(n) | Target::Struct(n) => n.clone(),
            Target::Variant { ty, variant } => format!("{}#{}", ty, variant),
            Target::Field(n) => format!(".{}", n),
        }
    }
}

/// A function's bytecode.
#[derive(Debug, Clone, PartialEq)]
pub struct Code {
    pub name: String,
    pub instrs: Vec<Instr>,
    /// How many slots the frame needs. Carried from the lowering pass, plus
    /// the two a `for` loop over a list keeps.
    pub frame_size: usize,
    pub pos: Pos,
}

/// A compiled program.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub code: Vec<Code>,
}

/// A compile failure, shaped like every other diagnostic.
#[derive(Debug, Clone, PartialEq)]
pub struct CompileError {
    pub message: String,
    pub pos: Pos,
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "error at {}: {}", self.pos, self.message)
    }
}

impl std::error::Error for CompileError {}

/// Compiles a lowered program to bytecode.
pub fn compile(program: &ir::Program) -> Result<Program, CompileError> {
    let mut out = Vec::new();
    for item in &program.items {
        if let ir::Item::Function(f) = item {
            out.push(function(f)?);
        }
    }
    Ok(Program { code: out })
}

/// The two extra slots a `for` loop over a list keeps: the sequence and the
/// index into it. They live above the frame the lowering pass sized, so a
/// nested loop gets its own pair.
const LOOP_SLOTS: usize = 2;

fn function(f: &ir::Fn) -> Result<Code, CompileError> {
    let mut c = FnCompiler {
        instrs: Vec::new(),
        loop_base: f.frame_size,
        loops: 0,
    };

    for stmt in &f.body.stmts {
        c.stmt(stmt)?;
    }
    match &f.body.tail {
        Some(t) => c.expr(t)?,
        None => c.emit(Op::Const(ir::Const::Int(0)), f.body.pos),
    };

    Ok(Code {
        name: f.name.clone(),
        instrs: c.instrs,
        frame_size: f.frame_size + LOOP_SLOTS,
        pos: f.pos,
    })
}

struct FnCompiler {
    instrs: Vec<Instr>,
    /// The first slot a loop over a list may use.
    loop_base: usize,
    /// How many such loops are currently open, so a nested loop gets its own
    /// slots rather than sharing its enclosing one's.
    loops: usize,
}

impl FnCompiler {
    fn emit(&mut self, op: Op, pos: Pos) {
        self.instrs.push(Instr { op, pos });
    }

    /// The instruction index just past everything emitted so far, which is
    /// where a jump with a target that is not known yet is patched.
    fn here(&self) -> usize {
        self.instrs.len()
    }

    fn patch(&mut self, at: usize, target: usize) {
        match &mut self.instrs[at].op {
            Op::Jump(t) | Op::JumpIfFalse(t) => *t = target,
            other => unreachable!("patch asked to patch a {:?}", other),
        }
    }

    fn block(&mut self, b: &ir::Block) -> Result<(), CompileError> {
        for stmt in &b.stmts {
            self.stmt(stmt)?;
        }
        match &b.tail {
            Some(t) => self.expr(t)?,
            None => self.emit(Op::Const(ir::Const::Int(0)), b.pos),
        }
        Ok(())
    }

    fn stmt(&mut self, s: &ir::Stmt) -> Result<(), CompileError> {
        match s {
            ir::Stmt::Let { slot, init, pos } => {
                self.expr(init)?;
                self.emit(Op::Store(*slot), *pos);
            }
            ir::Stmt::Assign { slot, value, pos } => {
                self.expr(value)?;
                self.emit(Op::Store(*slot), *pos);
            }
            ir::Stmt::Return(e) => {
                self.expr(e)?;
                self.emit(Op::Return, e.pos());
            }
            ir::Stmt::Nop { expr, pos } => {
                self.expr(expr)?;
                self.emit(Op::Pop, *pos);
            }
            ir::Stmt::Break => self.emit(Op::Break, Pos::START),
            ir::Stmt::Continue => self.emit(Op::Continue, Pos::START),

            ir::Stmt::While { cond, body, pos } => {
                let top = self.here();
                self.expr(cond)?;
                let exit = self.emit_at(Op::JumpIfFalse(0), *pos);
                self.loops += 1;
                self.block(body)?;
                self.loops -= 1;
                self.emit(Op::Jump(top), *pos);
                let end = self.here();
                self.patch(exit, end);
                self.emit(Op::Const(ir::Const::Int(0)), *pos);
            }

            ir::Stmt::For {
                var_slot,
                start,
                end,
                inclusive,
                body,
                pos,
            } => {
                // The end is stored one past the last value an inclusive range
                // reaches, so the loop condition is `<` in both forms.
                self.expr(start)?;
                self.emit(Op::Store(*var_slot), *pos);
                self.expr(end)?;
                if *inclusive {
                    self.emit(Op::Const(ir::Const::Int(1)), *pos);
                    self.emit(Op::Binary(ast::BinOp::Add), *pos);
                }
                // The limit goes in the pair of slots above the frame.
                let limit = self.loop_base + self.loops * 2;
                self.emit(Op::Store(limit + 1), *pos);

                let top = self.here();
                self.emit(Op::Load(*var_slot), *pos);
                self.emit(Op::Load(limit + 1), *pos);
                self.emit(Op::Binary(ast::BinOp::Lt), *pos);
                let exit = self.emit_at(Op::JumpIfFalse(0), *pos);

                self.loops += 1;
                self.block(body)?;
                self.loops -= 1;

                self.emit(Op::Load(*var_slot), *pos);
                self.emit(Op::Const(ir::Const::Int(1)), *pos);
                self.emit(Op::Binary(ast::BinOp::Add), *pos);
                self.emit(Op::Store(*var_slot), *pos);
                self.emit(Op::Jump(top), *pos);
                let end = self.here();
                self.patch(exit, end);
                self.emit(Op::Const(ir::Const::Int(0)), *pos);
            }

            ir::Stmt::ForEach {
                var_slot,
                iterable,
                body,
                pos,
            } => {
                let seq = self.loop_base + self.loops * 2;
                let idx = seq + 1;

                self.expr(iterable)?;
                self.emit(Op::Store(seq), *pos);
                self.emit(Op::Const(ir::Const::Int(0)), *pos);
                self.emit(Op::Store(idx), *pos);

                let top = self.here();
                self.emit(Op::Load(idx), *pos);
                // The index is compared against the length, which the VM puts
                // in slot `idx` when it walks a string rather than a list.
                self.emit(Op::Load(idx), *pos);
                self.emit(Op::Binary(ast::BinOp::Lt), *pos);
                let exit = self.emit_at(Op::JumpIfFalse(0), *pos);

                self.emit(Op::Load(seq), *pos);
                self.emit(Op::Load(idx), *pos);
                self.emit(Op::Index, *pos);
                self.emit(Op::Store(*var_slot), *pos);

                self.loops += 1;
                self.block(body)?;
                self.loops -= 1;

                self.emit(Op::Load(idx), *pos);
                self.emit(Op::Const(ir::Const::Int(1)), *pos);
                self.emit(Op::Binary(ast::BinOp::Add), *pos);
                self.emit(Op::Store(idx), *pos);
                self.emit(Op::Jump(top), *pos);
                let end = self.here();
                self.patch(exit, end);
                self.emit(Op::Const(ir::Const::Int(0)), *pos);
            }
        }
        Ok(())
    }

    fn emit_at(&mut self, op: Op, pos: Pos) -> usize {
        self.emit(op, pos);
        self.instrs.len() - 1
    }

    fn expr(&mut self, e: &ir::Expr) -> Result<(), CompileError> {
        let pos = e.pos();
        match e {
            ir::Expr::Const { value, .. } => self.emit(Op::Const(value.clone()), pos),
            ir::Expr::Load { slot, .. } => self.emit(Op::Load(*slot), pos),
            ir::Expr::Store { slot, value, .. } => {
                self.expr(value)?;
                self.emit(Op::Store(*slot), pos);
            }
            ir::Expr::Unary { neg, operand, .. } => {
                self.expr(operand)?;
                self.emit(Op::Unary { neg: *neg }, pos);
            }
            ir::Expr::Binary { op, lhs, rhs, .. } => {
                self.expr(lhs)?;
                self.expr(rhs)?;
                self.emit(Op::Binary(*op), pos);
            }
            ir::Expr::List { items, tuple, .. } => {
                for i in items {
                    self.expr(i)?;
                }
                self.emit(Op::BuildList { tuple: *tuple }, pos);
            }
            ir::Expr::Repeat { value, count, .. } => {
                self.expr(value)?;
                self.expr(count)?;
                self.emit(Op::Repeat, pos);
            }
            ir::Expr::Index { base, index, .. } => {
                self.expr(base)?;
                self.expr(index)?;
                self.emit(Op::Index, pos);
            }
            ir::Expr::IndexStore {
                slot, index, value, ..
            } => {
                self.emit(Op::Load(*slot), pos);
                self.expr(index)?;
                self.expr(value)?;
                self.emit(Op::IndexStore, pos);
            }
            ir::Expr::Field { base, name, .. } => {
                self.expr(base)?;
                self.emit(
                    Op::Call {
                        target: Target::Field(name.clone()),
                        arity: 1,
                    },
                    pos,
                );
            }
            ir::Expr::Try { inner, .. } => self.expr(inner)?,
            ir::Expr::Cast { value, to, .. } => {
                self.expr(value)?;
                self.emit(Op::Cast(to.clone()), pos);
            }
            ir::Expr::BlockValue(b) => {
                self.block(b)?;
                self.emit(Op::BlockValue, b.pos);
            }
            ir::Expr::If {
                cond,
                then,
                otherwise,
                ..
            } => {
                self.expr(cond)?;
                let els = self.emit_at(Op::JumpIfFalse(0), pos);
                self.block(then)?;
                match otherwise {
                    None => {
                        let end = self.here();
                        self.patch(els, end);
                        self.emit(Op::Const(ir::Const::Int(0)), pos);
                    }
                    Some(other) => {
                        let over = self.emit_at(Op::Jump(0), pos);
                        let else_at = self.here();
                        self.patch(els, else_at);
                        self.expr(other)?;
                        let end = self.here();
                        self.patch(over, end);
                    }
                }
            }
            ir::Expr::Match { arms, .. } => {
                // Each arm is tested in turn and its body jumps past the
                // remaining tests. The scrutinee stays on the stack so a test
                // can read it.
                let mut ends = Vec::new();
                for (i, arm) in arms.iter().enumerate() {
                    self.emit(Op::MatchTest { arm: i as u32 }, pos);
                    self.expr(&arm.body)?;
                    ends.push(self.emit_at(Op::Jump(0), pos));
                }
                self.emit(Op::MatchNone, pos);
                let end = self.here();
                for e in ends {
                    self.patch(e, end);
                }
            }
            ir::Expr::Call { target, args, .. } => {
                for a in args {
                    self.expr(a)?;
                }
                self.emit(
                    Op::Call {
                        target: to_target(target),
                        arity: args.len() as u32,
                    },
                    pos,
                );
            }
        }
        Ok(())
    }
}

fn to_target(t: &ir::CallTarget) -> Target {
    match t {
        ir::CallTarget::Function(n) => Target::Function(n.clone()),
        ir::CallTarget::Builtin(n) => Target::Builtin(n.clone()),
        ir::CallTarget::Struct(n) => Target::Struct(n.clone()),
        ir::CallTarget::Variant { ty, variant } => Target::Variant {
            ty: ty.clone(),
            variant: variant.clone(),
        },
    }
}

/// The instruction set as text, which is what the disassembly command prints.
impl Instr {
    pub fn describe(&self) -> String {
        match &self.op {
            Op::Const(ir::Const::Int(v)) => format!("const int {}", v),
            Op::Const(ir::Const::Float(v)) => format!("const float {}", v),
            Op::Const(ir::Const::Str(v)) => format!("const str {:?}", v),
            Op::Const(ir::Const::Char(v)) => format!("const char {:?}", v),
            Op::Const(ir::Const::Bool(v)) => format!("const bool {}", v),
            Op::Load(s) => format!("load s{}", s),
            Op::Store(s) => format!("store s{}", s),
            Op::Binary(op) => format!("{}", format!("{:?}", op).to_lowercase()),
            Op::Unary { neg } => format!("{}", if *neg { "neg" } else { "not" }),
            Op::Call { target, arity } => format!("call {}/{}", target.name(), arity),
            Op::Index => "index".to_string(),
            Op::IndexStore => "index-store".to_string(),
            Op::BuildList { tuple } => {
                format!("build-list {}", if *tuple { "tuple" } else { "array" })
            }
            Op::Repeat => "repeat".to_string(),
            Op::Cast(t) => format!("cast {}", t.name()),
            Op::JumpIfFalse(t) => format!("jump-false {}", t),
            Op::Jump(t) => format!("jump {}", t),
            Op::BlockValue => "block-value".to_string(),
            Op::Pop => "pop".to_string(),
            Op::Break => "break".to_string(),
            Op::Continue => "continue".to_string(),
            Op::Return => "return".to_string(),
            Op::MatchTest { arm } => format!("match-test {}", arm),
            Op::MatchNone => "match-none".to_string(),
        }
    }
}

/// Prints a compiled function, which is what makes the VM inspectable.
pub fn disassemble(code: &Code) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "fn {} at {} ({} slots, {} instructions)\n",
        code.name,
        code.pos,
        code.frame_size,
        code.instrs.len()
    ));
    for (i, instr) in code.instrs.iter().enumerate() {
        out.push_str(&format!(
            "  {:>4}  {:<28} ; {}\n",
            i,
            instr.describe(),
            instr.pos
        ));
    }
    out
}

/// Prints a whole compiled program.
pub fn disassemble_program(program: &Program) -> String {
    let mut out = String::new();
    for code in &program.code {
        out.push_str(&disassemble(code));
        out.push('\n');
    }
    out
}
