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
    /// How many values a following `BuildList` should gather. It is carried on
    /// the instruction itself so the VM does not have to count.
    ListLen(usize),
    /// Push a pattern for a following `MatchTest`.
    BuildPattern(Pattern),
    /// Push the position of a payload within a variant, used when a pattern
    /// binds one.
    PayloadIndex(usize),
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
    /// Read one named field of the value below.
    FieldRead(String),
    /// Leave a second copy of the value on top of the stack.
    Dup,

    /// The number of values in the collection below, which a `for` over a list
    /// needs for its condition.
    Len,
    /// Enter a loop body. The VM counts these so a `break` or `continue` knows
    /// it is inside a loop.
    LoopEnter,
    /// Leave a loop body, undoing a `LoopEnter`.
    LoopExit,
    /// End of a loop body. `on_break` is where a `break` jumps.
    EndLoop { on_break: usize },
    /// Test the value on top of the stack against a pattern. A true result
    /// falls through and leaves the subject on the stack when `keep` is set,
    /// and a false result jumps to `on_fail`.
    MatchTest { on_fail: usize, keep: bool },
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

/// A pattern, in the form the VM tests against.
///
/// The compiler flattens a pattern into this so the VM tests a value without
/// walking a tree, which is the same reason the rest of the program is
/// bytecode.
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    /// Matches anything.
    Wildcard,
    /// Matches a value equal to this.
    Literal(crate::ir::Const),
    /// Matches a list or tuple element by position.
    Tuple(Vec<Pattern>),
    /// Matches an enum value of this variant.
    Variant { ty: String, variant: String },
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
    /// The struct declarations, so an executor can build a struct without
    /// carrying the lowered form as well.
    pub structs: Vec<ir::StructDef>,
    /// The enum declarations, for the same reason.
    pub enums: Vec<ir::EnumDef>,
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
    let mut code = Vec::new();
    let mut structs = Vec::new();
    let mut enums = Vec::new();
    for item in &program.items {
        match item {
            ir::Item::Function(f) => code.push(function(f)?),
            ir::Item::Struct(s) => structs.push(s.clone()),
            ir::Item::Enum(e) => enums.push(e.clone()),
        }
    }
    Ok(Program {
        code,
        structs,
        enums,
    })
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
        exits: Vec::new(),
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

/// Which kind of loop exit a jump is.
///
/// A `continue` has to run the counter increment and a `break` has to skip it,
/// so the two land in different places and the compiler has to know which is
/// which while it patches them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExitKind {
    Break,
    Continue,
}

struct FnCompiler {
    instrs: Vec<Instr>,
    /// The jump of each `break` or `continue` in the loop currently being
    /// compiled, one list per open loop so a nested loop keeps its own.
    exits: Vec<Vec<(usize, ExitKind)>>,
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

    /// Records a break or continue jump for the innermost open loop.
    fn pending_exit(&mut self, at: usize, kind: ExitKind) {
        if let Some(loop_exits) = self.exits.last_mut() {
            loop_exits.push((at, kind));
        }
    }

    /// Points every `continue` of the innermost loop at its EndLoop, so the
    /// counter has already moved when the flag is consumed.
    fn close_continues(&mut self, target: usize) {
        if let Some(loop_exits) = self.exits.last_mut() {
            let continues: Vec<usize> = loop_exits
                .iter()
                .filter(|(_, kind)| *kind == ExitKind::Continue)
                .map(|(at, _)| *at)
                .collect();
            for at in continues {
                if let Op::Jump(t) = &mut self.instrs[at].op {
                    if *t == usize::MAX {
                        *t = target;
                    }
                }
            }
        }
    }

    /// Points every `break` of the innermost loop past the increment, and
    /// forgets the loop's exits.
    fn close_breaks(&mut self, target: usize) {
        if let Some(loop_exits) = self.exits.pop() {
            for (at, kind) in loop_exits {
                if kind == ExitKind::Break {
                    if let Op::Jump(t) = &mut self.instrs[at].op {
                        if *t == usize::MAX {
                            *t = target;
                        }
                    }
                }
            }
        }
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
            ir::Stmt::Break => {
                let at = self.emit_at(Op::Break, Pos::START);
                // Nothing jumps on a flag alone, so the rest of the body would
                // still run. Both exit instructions jump to the loop's EndLoop,
                // which is what consumes the flag.
                self.emit(Op::Jump(usize::MAX), Pos::START);
                self.pending_exit(at + 1, ExitKind::Break);
            }
            ir::Stmt::Continue => {
                let at = self.emit_at(Op::Continue, Pos::START);
                self.emit(Op::Jump(usize::MAX), Pos::START);
                self.pending_exit(at + 1, ExitKind::Continue);
            }

            ir::Stmt::While { cond, body, pos } => {
                let top = self.here();
                self.expr(cond)?;
                let exit = self.emit_at(Op::JumpIfFalse(0), *pos);
                self.emit(Op::LoopEnter, *pos);
                self.loops += 1;
                self.exits.push(Vec::new());
                self.block(body)?;
                self.loops -= 1;
                self.emit(Op::EndLoop { on_break: 0 }, *pos);
                self.emit(Op::Jump(top), *pos);
                let end = self.here();
                self.patch(exit, end);
                self.patch_loop_break(self.instrs.len() - 3, end);
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

                self.emit(Op::LoopEnter, *pos);
                self.loops += 1;
                self.exits.push(Vec::new());
                self.block(body)?;
                self.loops -= 1;

                // The increment comes before EndLoop so that a continue, which
                // lands on EndLoop, still moves the counter on. A break jumps
                // past the increment instead, so the counter is left alone and
                // the loop condition stops it.
                self.emit(Op::Load(*var_slot), *pos);
                self.emit(Op::Const(ir::Const::Int(1)), *pos);
                self.emit(Op::Binary(ast::BinOp::Add), *pos);
                self.emit(Op::Store(*var_slot), *pos);

                let end_loop = self.emit_at(Op::EndLoop { on_break: 0 }, *pos);
                self.close_continues(end_loop);

                self.emit(Op::Jump(top), *pos);
                let after = self.here();
                self.close_breaks(after);

                let end = self.here();
                self.patch(exit, end);
                self.patch_loop_break(end_loop, end);
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
                // The condition is index < length of the sequence. It compared
                // the index with itself before, which is never true, so the loop
                // either never ran or never stopped.
                self.emit(Op::Load(idx), *pos);
                self.emit(Op::Load(seq), *pos);
                self.emit(Op::Len, *pos);
                self.emit(Op::Binary(ast::BinOp::Lt), *pos);
                let exit = self.emit_at(Op::JumpIfFalse(0), *pos);

                self.emit(Op::LoopEnter, *pos);
                self.emit(Op::Load(seq), *pos);
                self.emit(Op::Load(idx), *pos);
                self.emit(Op::Index, *pos);
                self.emit(Op::Store(*var_slot), *pos);

                self.loops += 1;
                self.block(body)?;
                self.loops -= 1;
                self.emit(Op::EndLoop { on_break: 0 }, *pos);

                self.emit(Op::Load(idx), *pos);
                self.emit(Op::Const(ir::Const::Int(1)), *pos);
                self.emit(Op::Binary(ast::BinOp::Add), *pos);
                self.emit(Op::Store(idx), *pos);
                self.emit(Op::Jump(top), *pos);
                let end = self.here();
                self.patch(exit, end);
                self.patch_loop_break(self.instrs.len() - 4, end);
                self.emit(Op::Const(ir::Const::Int(0)), *pos);
            }
        }
        Ok(())
    }

    /// Points the `EndLoop` at `at` back at `target`, which is where a break
    /// leaves the loop.
    fn patch_loop_break(&mut self, at: usize, target: usize) {
        if let Op::EndLoop { on_break } = &mut self.instrs[at].op {
            *on_break = target;
        }
    }

    /// Points a failed pattern test at `target`.
    fn patch_match_fail(&mut self, at: usize, target: usize) {
        if let Op::MatchTest { on_fail, .. } = &mut self.instrs[at].op {
            *on_fail = target;
        }
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
                self.emit(Op::ListLen(items.len()), pos);
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
                self.emit(Op::FieldRead(name.clone()), pos);
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
            ir::Expr::Match {
                scrutinee, arms, ..
            } => {
                // The scrutinee is duplicated for each arm, because a test
                // consumes the copy it reads. Duplicating costs one
                // instruction per arm and removes the need for the VM to know
                // how deep on the stack a scrutinee sits.
                let mut ends = Vec::new();
                for arm in arms.iter() {
                    // A pattern that binds names needs the matched value
                    // afterwards, so a test that succeeds leaves the subject on
                    // the stack. A pattern that binds nothing does not, which
                    // is why the instruction carries whether to keep it.
                    self.expr(scrutinee)?;
                    self.emit(Op::BuildPattern(pattern_of(&arm.pattern)), pos);
                    let test = self.emit_at(
                        Op::MatchTest {
                            on_fail: 0,
                            keep: binds(&arm.pattern),
                        },
                        pos,
                    );
                    // Each payload is read by position, so the index is pushed
                    // before the read rather than kept in a slot.
                    match &arm.pattern {
                        // A pattern that binds the whole value is a plain store.
                        ir::Pattern::Binding(b) => self.emit(Op::Store(b.slot), pos),
                        // A variant pattern reads its payloads by position.
                        ir::Pattern::Variant { bindings, .. } => {
                            // A payload read consumes the subject and leaves
                            // only the payload, which the store then consumes
                            // too. So every read but the last duplicates the
                            // subject first, keeping one copy for the read
                            // that follows.
                            for (i, b) in bindings.iter().enumerate() {
                                if i + 1 < bindings.len() {
                                    self.emit(Op::Dup, pos);
                                }
                                self.emit(Op::PayloadIndex(i), pos);
                                self.emit(Op::Store(b.slot), pos);
                            }
                        }
                        _ => {}
                    }
                    self.expr(&arm.body)?;
                    ends.push(self.emit_at(Op::Jump(0), pos));
                    let next = self.here();
                    self.patch_match_fail(test, next);
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

/// Whether a pattern binds names, and so needs the subject kept.
fn binds(p: &ir::Pattern) -> bool {
    match p {
        ir::Pattern::Binding(_) => true,
        ir::Pattern::Variant { bindings, .. } => !bindings.is_empty(),
        _ => false,
    }
}

/// Flattens a lowered pattern into the form the VM tests against.
fn pattern_of(p: &ir::Pattern) -> Pattern {
    match p {
        ir::Pattern::Wildcard => Pattern::Wildcard,
        ir::Pattern::Literal(c) => Pattern::Literal(c.clone()),
        ir::Pattern::Tuple(items) => Pattern::Tuple(items.iter().map(pattern_of).collect()),
        ir::Pattern::Binding(_) => Pattern::Wildcard,
        ir::Pattern::Variant { ty, variant, .. } => Pattern::Variant {
            ty: ty.clone(),
            variant: variant.clone(),
        },
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
            Op::ListLen(n) => format!("list-len {}", n),
            Op::BuildPattern(p) => format!("build-pattern {}", pattern_text(p)),
            Op::FieldRead(n) => format!("field-read {}", n),
            Op::Dup => "dup".to_string(),
            Op::Len => "len".to_string(),
            Op::LoopEnter => "loop-enter".to_string(),
            Op::LoopExit => "loop-exit".to_string(),
            Op::EndLoop { on_break } => format!("end-loop break->{}", on_break),
            Op::MatchTest { on_fail, keep } => {
                format!(
                    "match-test fail->{}{}",
                    on_fail,
                    if *keep { " keep" } else { "" }
                )
            }
            Op::PayloadIndex(n) => format!("payload-index {}", n),
            Op::MatchNone => "match-none".to_string(),
        }
    }
}

/// A constant as text, for the disassembly command.
pub fn const_text(c: &ir::Const) -> String {
    match c {
        ir::Const::Int(v) => format!("int {}", v),
        ir::Const::Float(v) => format!("float {}", v),
        ir::Const::Str(v) => format!("str {:?}", v),
        ir::Const::Char(v) => format!("char {:?}", v),
        ir::Const::Bool(v) => format!("bool {}", v),
    }
}

/// A pattern as text, for the disassembly command.
pub fn pattern_text(p: &Pattern) -> String {
    match p {
        Pattern::Wildcard => "_".to_string(),
        Pattern::Literal(c) => const_text(c),
        Pattern::Tuple(items) => {
            let parts: Vec<String> = items.iter().map(pattern_text).collect();
            format!("({})", parts.join(", "))
        }
        Pattern::Variant { ty, variant } => format!("{}.{}", ty, variant),
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
