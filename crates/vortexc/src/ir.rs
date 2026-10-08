//! The lowered form of a parsed program.
//!
//! `SPEC.md` section 10.1 asks for the AST to be lowered once, in stage 2, so
//! that the tree interpreter in this stage and the bytecode VM in stage 4
//! consume the same thing. Stage 4 then adds a consumer rather than rewriting
//! the frontend.
//!
//! What lowering does:
//!
//! - Every name is resolved to a slot index, so an executor looks a variable up
//!   by index rather than by comparing strings. A name that is not in scope, an
//!   assignment to an immutable binding, and a call to a function that was never
//!   declared are all errors here, reported with the position the parser
//!   recorded.
//! - Every type known at compile time is decided here. A struct literal becomes
//!   a constructor call and an enum value becomes a variant call, so an executor
//!   carries one call case rather than one per syntactic form.
//! - Every expression is written to a slot before its parent reads it, and every
//!   position is carried through, so a run time error names the line and column
//!   the programmer wrote.

use crate::ast;
use crate::span::Pos;

/// A lowered program.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Function(Fn),
    Struct(StructDef),
    Enum(EnumDef),
}

/// A function.
#[derive(Debug, Clone, PartialEq)]
pub struct Fn {
    pub name: String,
    pub params: Vec<Param>,
    /// The source level return type, for stage 3 to check against.
    pub ret: Option<ast::TypeExpr>,
    pub body: Block,
    /// How many slots the frame needs, including parameters.
    pub frame_size: usize,
    pub pos: Pos,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    /// The slot the argument is bound to.
    pub slot: Slot,
    pub name: String,
    pub mutable: bool,
    pub ty: ast::TypeExpr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<ast::Field>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumDef {
    pub name: String,
    pub variants: Vec<ast::Variant>,
}

/// An index into a call frame.
pub type Slot = usize;

/// A lowered block.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    /// The trailing expression. Boxed for the same reason as `ast::Block`.
    pub tail: Option<Box<Expr>>,
    pub pos: Pos,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Let {
        slot: Slot,
        init: Expr,
        pos: Pos,
    },
    Assign {
        slot: Slot,
        value: Expr,
        pos: Pos,
    },
    Return(Expr),
    While {
        cond: Expr,
        body: Block,
        pos: Pos,
    },
    /// `for x in a..=b`, which counts.
    For {
        var_slot: Slot,
        start: Expr,
        end: Expr,
        inclusive: bool,
        body: Block,
        pos: Pos,
    },
    /// `for x in e`, which walks a list or a string.
    ForEach {
        var_slot: Slot,
        iterable: Expr,
        body: Block,
        pos: Pos,
    },
    Break,
    Continue,
    /// A statement that produces no value, such as a call written for its
    /// effect. It is kept so an executor walks the same shape the lowering
    /// produced.
    Nop {
        expr: Expr,
        pos: Pos,
    },
}

/// A lowered expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Load {
        slot: Slot,
        pos: Pos,
    },
    Store {
        slot: Slot,
        value: Box<Expr>,
        pos: Pos,
    },
    Const {
        value: Const,
        pos: Pos,
    },
    Binary {
        op: ast::BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        pos: Pos,
    },
    Unary {
        neg: bool,
        operand: Box<Expr>,
        pos: Pos,
    },
    /// Call a function, a builtin, a struct constructor or an enum variant
    /// constructor. All four share this one case, so an executor has a single
    /// call path.
    ///
    /// A call to a declared function takes `arg_slots` as well: the callee has
    /// its own frame, so the caller evaluates each argument into a slot of its
    /// own and the callee reads them from there. A builtin or a constructor has
    /// no frame, so `arg_slots` is empty and the values are passed directly.
    Call {
        target: CallTarget,
        args: Vec<Expr>,
        arg_slots: Vec<Slot>,
        pos: Pos,
    },
    If {
        cond: Box<Expr>,
        then: Box<Block>,
        otherwise: Option<Box<Expr>>,
        pos: Pos,
    },
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<Arm>,
        pos: Pos,
    },
    /// An array or a tuple. `tuple` records which, because `[]` and `()` print
    /// differently.
    List {
        items: Vec<Expr>,
        tuple: bool,
        pos: Pos,
    },
    /// `e as T`, an explicit conversion. `to` is the source level type, so an
    /// executor can decide the conversion without a type table.
    Cast {
        value: Box<Expr>,
        to: ast::TypeExpr,
        pos: Pos,
    },
    /// `[value; count]`, a list of `count` copies of `value`.
    ///
    /// `SPEC.md` section 8.2 recorded a list of a computed length as missing in
    /// v0.1. This is the form that adds it.
    Repeat {
        value: Box<Expr>,
        count: Box<Expr>,
        pos: Pos,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
        /// The element type of the list being indexed.
        ///
        /// The index expression does not carry it: `a[i]` has an `Int` index
        /// whatever `a` holds, so recovering it from the syntax guesses from the
        /// wrong fact. The lowering pass resolved the list when it allocated
        /// the slot, so it records the element type here and a consumer reads
        /// it rather than inferring it.
        element: Option<crate::lower::Element>,
        pos: Pos,
    },
    /// Store into a list element, as `a[i] = v`.
    ///
    /// The list is named by the frame slot that holds it rather than by an
    /// expression, because the executor has to write the mutated list back into
    /// that slot. Mutating a value read out of a slot would otherwise change a
    /// copy and lose the store.
    IndexStore {
        slot: Slot,
        index: Box<Expr>,
        value: Box<Expr>,
        pos: Pos,
    },
    Field {
        base: Box<Expr>,
        name: String,
        pos: Pos,
    },
    Try {
        inner: Box<Expr>,
        pos: Pos,
    },
    /// A block used as a value, which is how an `else` branch is represented.
    BlockValue(Box<Block>),
}

impl Expr {
    /// The position this expression started at, which a diagnostic reports.
    pub fn pos(&self) -> Pos {
        match self {
            Expr::Load { pos, .. }
            | Expr::Const { pos, .. }
            | Expr::Store { pos, .. }
            | Expr::Binary { pos, .. }
            | Expr::Unary { pos, .. }
            | Expr::Call { pos, .. }
            | Expr::If { pos, .. }
            | Expr::Match { pos, .. }
            | Expr::List { pos, .. }
            | Expr::Repeat { pos, .. }
            | Expr::Cast { pos, .. }
            | Expr::Index { pos, .. }
            | Expr::IndexStore { pos, .. }
            | Expr::Field { pos, .. }
            | Expr::Try { pos, .. } => *pos,
            Expr::BlockValue(b) => b.pos,
        }
    }

    /// A constant value this expression reduces to at compile time, if it is
    /// built entirely from literals.
    ///
    /// The bytecode compiler folds a constant `Binary` or unary negation into a
    /// single `Const`, so `2 + 3 * 4` becomes `Const(14)` instead of two loads, a
    /// multiply and an add. Only arithmetic the tree interpreter also treats as
    /// pure is folded: the two arithmetic rules stay in step with each other.
    pub fn const_value(&self) -> Option<Const> {
        match self {
            Expr::Const { value, .. } => Some(value.clone()),
            // `not` is folded nowhere: the runtime owns it, so the fold must not
            // disagree with it. Arithmetic negation is folded because `2 + -3*4`
            // then starts from constants the same way the interpreter would.
            Expr::Unary { neg, operand, .. } => {
                let v = operand.const_value()?;
                Some(match (neg, v) {
                    (true, Const::Int(n)) => Const::Int(n.wrapping_neg()),
                    (true, Const::Float(n)) => Const::Float(-n),
                    _ => return None,
                })
            }
            Expr::Binary { op, lhs, rhs, .. } => {
                let l = lhs.const_value()?;
                let r = rhs.const_value()?;
                fold_binary(*op, l, r)
            }
            _ => None,
        }
    }
}

/// Applies a binary operator to two constant values, the arithmetic matching
/// the tree interpreter exactly: integers wrap, floats are real arithmetic.
///
/// Returns `None` for a type mismatch such as `Str + Int`, and for a division or
/// remainder whose divisor is zero. The runtime raises `DivideByZero` for those,
/// so the compiler must not pretend to know a result it would never compute.
fn fold_binary(op: ast::BinOp, l: Const, r: Const) -> Option<Const> {
    use ast::BinOp::*;
    use Const::*;
    match (op, &l, &r) {
        (Add, Int(a), Int(b)) => Some(Int(a.wrapping_add(*b))),
        (Sub, Int(a), Int(b)) => Some(Int(a.wrapping_sub(*b))),
        (Mul, Int(a), Int(b)) => Some(Int(a.wrapping_mul(*b))),
        (Div, Int(a), Int(b)) if *b != 0 => Some(Int(a.wrapping_div(*b))),
        (Rem, Int(a), Int(b)) if *b != 0 => Some(Int(a.wrapping_rem(*b))),
        (Add, Float(a), Float(b)) => Some(Float(a + b)),
        (Sub, Float(a), Float(b)) => Some(Float(a - b)),
        (Mul, Float(a), Float(b)) => Some(Float(a * b)),
        (Div, Float(a), Float(b)) if *b != 0.0 => Some(Float(a / b)),
        _ => None,
    }
}

/// What a call refers to, decided at lowering time.
#[derive(Debug, Clone, PartialEq)]
pub enum CallTarget {
    /// A function declared in the program.
    Function(String),
    /// A builtin such as `println`.
    Builtin(String),
    /// `Point(..)`, which builds a struct.
    Struct(String),
    /// `Shape.circle(..)`, which builds an enum value.
    Variant { ty: String, variant: String },
}

/// A lowered `match` arm.
#[derive(Debug, Clone, PartialEq)]
pub struct Arm {
    pub pattern: Pattern,
    pub body: Expr,
}

/// A lowered pattern.
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Wildcard,
    Tuple(Vec<Pattern>),
    Literal(Const),
    Variant {
        ty: String,
        variant: String,
        bindings: Vec<Binding>,
    },
    /// Binds the matched value to a slot and always matches.
    Binding(Binding),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    pub slot: Slot,
    pub name: String,
}

/// A value known at lowering time.
#[derive(Debug, Clone, PartialEq)]
pub enum Const {
    Int(i64),
    Float(f64),
    Str(String),
    Char(char),
    Bool(bool),
}

/// A lowering error.
#[derive(Debug, Clone, PartialEq)]
pub struct LowerError {
    pub message: String,
    pub pos: Pos,
}

impl std::fmt::Display for LowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "error at {}: {}", self.pos, self.message)
    }
}

impl std::error::Error for LowerError {}
