//! Runtime values.
//!
//! Vortex v0.1 keeps values in a tree walk, so a value is an owned Rust value
//! rather than a boxed, reference counted cell. When stage 4 introduces a heap,
//! the `Value::Ref` variant becomes a handle into it and nothing else changes.

use crate::span::Pos;

/// A runtime value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Char(char),
    /// An ordered list, written `[a, b, c]`.
    Array(Vec<Value>),
    /// A tuple, written `(a, b)`.
    Tuple(Vec<Value>),
    /// A struct value. Field order matches the declaration.
    Struct {
        name: String,
        fields: Vec<(String, Value)>,
    },
    /// An enum value. `args` holds positional payloads or named fields.
    Variant {
        ty: String,
        variant: String,
        args: Vec<Value>,
    },
    /// A function, or a reference to a builtin.
    Func(String),
    /// An absent value, which is how `none` is represented.
    Unit,
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_) => "Int",
            Value::Float(_) => "Float",
            Value::Bool(_) => "Bool",
            Value::Str(_) => "Str",
            Value::Char(_) => "Char",
            Value::Array(_) => "Array",
            Value::Tuple(_) => "Tuple",
            Value::Struct { .. } => "Struct",
            Value::Variant { .. } => "Enum",
            Value::Func(_) => "Fn",
            Value::Unit => "()",
        }
    }

    pub fn truthy(&self) -> Result<bool, String> {
        match self {
            Value::Bool(b) => Ok(*b),
            other => Err(format!(
                "expected a Bool in a condition, found {}",
                other.type_name()
            )),
        }
    }
}

/// A run time error. Every variant carries a message a reader can act on.
#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeError {
    /// A name was not in scope when the program ran.
    Undefined { name: String, pos: Pos },
    /// A name was used as a callee but holds something other than a function.
    NotCallable { name: String, pos: Pos },
    /// An operator or builtin was applied to values it does not accept.
    /// `lhs` and `rhs` name the types that were found, and are empty when the
    /// operation only takes one value.
    BadOperands {
        op: String,
        lhs: String,
        rhs: String,
        pos: Pos,
    },
    /// An index was outside a collection, or the collection was not indexable.
    BadIndex { pos: Pos, detail: String },
    /// A field name that the value does not have.
    UnknownField { name: String, pos: Pos },
    /// A field read on something that is not a struct or enum.
    NotAStruct { pos: Pos, found: String },
    /// `?` on a value that is not an `Err`.
    NothingToPropagate { pos: Pos, found: String },
    /// Integer division or remainder by zero.
    DivideByZero { pos: Pos },
    /// A `break` or `continue` outside a loop, or a bare `return` with a value
    /// in a function that has no return type.
    BadControlFlow { pos: Pos, detail: String },
    /// A call to a function that was never declared.
    UnknownFunction { name: String, pos: Pos },
    /// A builtin called with the wrong number of arguments.
    BadArity {
        name: String,
        want: usize,
        got: usize,
        pos: Pos,
    },
    /// Division of two integers that does not divide evenly.
    NotExact { pos: Pos },
    /// A `main` that did not finish by returning from itself.
    MainDidNotReturn { pos: Pos },
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeError::Undefined { name, pos } => {
                write!(f, "error at {}: undefined name `{}`", pos, name)
            }
            RuntimeError::NotCallable { name, pos } => write!(
                f,
                "error at {}: `{}` is not a function and cannot be called",
                pos, name
            ),
            RuntimeError::BadOperands { op, lhs, rhs, pos } => {
                if rhs.is_empty() {
                    write!(f, "error at {}: `{}` cannot be applied to {}", pos, op, lhs)
                } else {
                    write!(
                        f,
                        "error at {}: `{}` cannot be applied to {} and {}",
                        pos, op, lhs, rhs
                    )
                }
            }
            RuntimeError::BadIndex { pos, detail } => {
                write!(f, "error at {}: bad index: {}", pos, detail)
            }
            RuntimeError::UnknownField { name, pos } => {
                write!(f, "error at {}: this value has no field `{}`", pos, name)
            }
            RuntimeError::NotAStruct { pos, found } => write!(
                f,
                "error at {}: cannot read a field of {}, which is not a struct or enum",
                pos, found
            ),
            RuntimeError::NothingToPropagate { pos, found } => write!(
                f,
                "error at {}: `?` needs an Err to propagate, found {}",
                pos, found
            ),
            RuntimeError::DivideByZero { pos } => {
                write!(f, "error at {}: division by zero", pos)
            }
            RuntimeError::BadControlFlow { pos, detail } => {
                write!(f, "error at {}: {}", pos, detail)
            }
            RuntimeError::UnknownFunction { name, pos } => {
                write!(f, "error at {}: call to unknown function `{}`", pos, name)
            }
            RuntimeError::BadArity {
                name,
                want,
                got,
                pos,
            } => write!(
                f,
                "error at {}: `{}` takes {} argument{}, got {}",
                pos,
                name,
                want,
                if *want == 1 { "" } else { "s" },
                got
            ),
            RuntimeError::NotExact { pos } => write!(
                f,
                "error at {}: integer division must divide evenly; cast with `as Float`",
                pos
            ),
            RuntimeError::MainDidNotReturn { pos } => write!(
                f,
                "error at {}: `main` reached the end without a `return`",
                pos
            ),
        }
    }
}

impl std::error::Error for RuntimeError {}

/// The result of running an expression or block.
pub type EvalResult = Result<Value, RuntimeError>;
