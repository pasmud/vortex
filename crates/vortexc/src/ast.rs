//! The surface syntax tree produced by the parser.
//!
//! This is the shape a programmer writes. It is close to the source and keeps
//! every piece of syntax. It is lowered into the `ir` form before anything
//! executes.
//!
//! Every node carries the position it started at, so that a later stage can
//! report an error about a name, a call or a binding with the line and column
//! the programmer wrote it on, as `SPEC.md` section 1 requires.

use crate::span::Pos;

/// A node in the tree, with the position it started at.
#[derive(Debug, Clone, PartialEq)]
pub struct Spanned<T> {
    pub node: T,
    pub pos: Pos,
}

impl<T> Spanned<T> {
    pub fn new(node: T, pos: Pos) -> Self {
        Spanned { node, pos }
    }
}

/// A whole source file.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub items: Vec<Item>,
}

/// A top level item.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Function(Spanned<FnDecl>),
    Struct(Spanned<StructDecl>),
    Enum(Spanned<EnumDecl>),
}

/// A function declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct FnDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub ret: Option<TypeExpr>,
    pub body: Block,
}

/// A parameter. `SPEC.md` section 8 says a parameter is immutable unless it is
/// written `var`.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: TypeExpr,
    pub mutable: bool,
}

/// A type as written.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    /// A named type such as `Int` or `Point`, possibly `Shape.rect`.
    Named(String),
}

impl TypeExpr {
    pub fn name(&self) -> &str {
        match self {
            TypeExpr::Named(n) => n,
        }
    }
}

/// A struct declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct StructDecl {
    pub name: String,
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: String,
    pub ty: TypeExpr,
}

/// An enum declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct EnumDecl {
    pub name: String,
    pub variants: Vec<Variant>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    pub name: String,
    /// Positional payloads, as in `circle(Float)`.
    pub payloads: Vec<TypeExpr>,
    /// Named fields, as in `rect { w: Float, h: Float }`.
    pub named: Vec<Field>,
    pub pos: Pos,
}

/// A statement, with the position it started at.
#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub pos: Pos,
    pub kind: StmtKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    /// `let x = 1;`
    Let {
        name: String,
        mutable: bool,
        init: Expr,
    },
    /// `return e;`
    Return(Expr),
    /// A bare expression statement, which is also how an assignment is written.
    Expr(Expr),
    /// A `{ ... }` used where a statement is expected.
    Block(Block),
    /// `while c { ... }`
    While { cond: Expr, body: Block },
    /// `for x in a..=b { ... }`, which counts, or `for x in e { ... }`, which
    /// walks a list or a string. `end` is `None` for the second form.
    For {
        var: String,
        start: Expr,
        end: Option<Expr>,
        inclusive: bool,
        body: Block,
    },
    Break,
    Continue,
}

/// A block. Its value is the trailing expression when there is one, which
/// `SPEC.md` section 8 makes an expression.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub pos: Pos,
    pub stmts: Vec<Stmt>,
    /// Boxed because a block holds its trailing expression and an expression
    /// can hold a block, so an unboxed tail would make the type infinite.
    pub tail: Option<Box<Expr>>,
}

/// An expression, with the position it started at.
#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub pos: Pos,
    pub kind: ExprKind,
}

impl Expr {
    pub fn new(pos: Pos, kind: ExprKind) -> Self {
        Expr { pos, kind }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Int(i64),
    Float(f64),
    Str(String),
    Char(char),
    Bool(bool),
    /// A bare name: a variable read, or a call with no arguments.
    Ident(String),
    /// `f(a, b)`
    Call { callee: String, args: Vec<Expr> },
    /// `-e`
    Neg(Box<Expr>),
    /// `!e`
    Not(Box<Expr>),
    /// `name = value`, which is an expression statement rather than a binary
    /// operator, because the left side is a name and not a value.
    Assign { name: String, value: Box<Expr> },
    /// `lhs op rhs`
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// `if c { .. } else { .. }`
    If {
        cond: Box<Expr>,
        then: Block,
        otherwise: Option<Box<Else>>,
    },
    /// `match e { .. }`
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<Arm>,
    },
    /// `Point { x: 1, y: 2 }`
    Record {
        ty: String,
        fields: Vec<(String, Expr)>,
    },
    /// `Shape.empty`
    Variant { ty: String, variant: String },
    /// `Shape.circle(2.0)`
    VariantCall {
        ty: String,
        variant: String,
        args: Vec<Expr>,
    },
    /// `Shape.rect { w: 2.0, h: 3.0 }`
    VariantRecord {
        ty: String,
        variant: String,
        fields: Vec<(String, Expr)>,
    },
    /// `(a, b)`
    Tuple(Vec<Expr>),
    /// `(e)`, kept so the tree matches the source.
    Paren(Box<Expr>),
    /// `{ .. }` used as a value rather than as a statement.
    Block(Block),
    /// `e?`
    Try(Box<Expr>),
    /// `[a, b, c]`
    Array(Vec<Expr>),
    /// `e[i]`
    Index(Box<Expr>, Box<Expr>),
    /// `e.f`
    Field(Box<Expr>, String),
}

/// The `else` part of an if expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Else {
    Block(Block),
    If(Spanned<Expr>),
}

/// One arm of a `match`. Several alternatives share one body.
#[derive(Debug, Clone, PartialEq)]
pub struct Arm {
    pub pos: Pos,
    pub patterns: Vec<Pattern>,
    pub body: Expr,
}

/// A pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct Pattern {
    pub pos: Pos,
    pub kind: PatternKind,
}

impl Pattern {
    pub fn new(pos: Pos, kind: PatternKind) -> Self {
        Pattern { pos, kind }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PatternKind {
    /// `_`
    Wildcard,
    /// `(0, _)`
    Tuple(Vec<Pattern>),
    /// A constant compared for equality.
    Literal(Expr),
    /// `Shape.circle(r)`
    Variant {
        ty: String,
        variant: String,
        /// Names bound by the pattern, in order.
        bindings: Vec<String>,
    },
    /// A name, which matches anything and introduces a binding.
    Binding(String),
}

/// A binary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

impl BinOp {
    pub fn spelling(&self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::And => "&&",
            BinOp::Or => "||",
        }
    }
}

/// The statement keywords, from `SPEC.md` section 5.3.
pub fn is_keyword(name: &str) -> bool {
    matches!(
        name,
        "as" | "break"
            | "continue"
            | "else"
            | "enum"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "import"
            | "in"
            | "let"
            | "match"
            | "return"
            | "self"
            | "struct"
            | "trait"
            | "true"
            | "var"
            | "while"
    )
}
