//! Vortex types.
//!
//! The types `SPEC.md` section 6 names, and the rules for combining them. A
//! type is either one of the built in types or a name the program declared, so
//! two structs with identical fields are different types, which is what
//! section 6.1 rule 2 means by nominal.

use std::fmt;

/// A type.
///
/// `Option<T>` and `Result<T, E>` are built in rather than declared, because
/// section 6.1 rules 5 and 6 give them their own notation. Everything else that
/// has a name was declared by the program as a `struct` or an `enum`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Int,
    Float,
    Bool,
    Str,
    Char,
    /// The type of a statement that produces no value, such as a `let`.
    Unit,
    /// `[T]`
    Array(Box<Type>),
    /// `(A, B)`. An empty tuple is `()`.
    Tuple(Vec<Type>),
    /// A named type. `id` is the index of the declaration, so two structs with
    /// the same fields are different types.
    Named(NamedId),
    /// `Option<T>`, written `none` or `some(x)`.
    Option(Box<Type>),
    /// `Result<T, E>`, written `ok(v)` or `err(e)`.
    Result(Box<Type>, Box<Type>),
}

/// Identifies a declared struct or enum. Two names are never equal, so this is
/// what makes the typing nominal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NamedId(pub usize);

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Int => write!(f, "Int"),
            Type::Float => write!(f, "Float"),
            Type::Bool => write!(f, "Bool"),
            Type::Str => write!(f, "Str"),
            Type::Char => write!(f, "Char"),
            Type::Unit => write!(f, "()"),
            Type::Array(t) => write!(f, "[{}]", t),
            Type::Tuple(ts) => {
                write!(f, "(")?;
                for (i, t) in ts.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", t)?;
                }
                write!(f, ")")
            }
            Type::Named(_) => write!(f, "a declared type"),
            Type::Option(t) => write!(f, "Option<{}>", t),
            Type::Result(ok, err) => write!(f, "Result<{}, {}>", ok, err),
        }
    }
}

/// The declared structs and enums, in declaration order.
///
/// `NamedId` indexes this, which is what makes the typing nominal: two
/// declarations are two ids and never the same type.
#[derive(Debug, Clone, Default)]
pub struct Decls {
    entries: Vec<Decl>,
}

#[derive(Debug, Clone)]
pub struct Decl {
    pub name: String,
    /// The fields of a struct, empty for an enum.
    pub fields: Vec<(String, Type)>,
    /// The variants of an enum, empty for a struct.
    pub variants: Vec<VariantDecl>,
    pub is_enum: bool,
}

#[derive(Debug, Clone)]
pub struct VariantDecl {
    pub name: String,
    /// The type of each payload, positional first then named, so a payload can
    /// be read by index.
    pub payloads: Vec<Type>,
    /// The names of named fields, parallel to `payloads`.
    pub named: Vec<String>,
}

impl Decls {
    pub fn new() -> Self {
        Decls::default()
    }

    /// Declares a struct, returning its id.
    pub fn add_struct(&mut self, name: &str, fields: Vec<(String, Type)>) -> NamedId {
        self.entries.push(Decl {
            name: name.to_string(),
            fields,
            variants: Vec::new(),
            is_enum: false,
        });
        NamedId(self.entries.len() - 1)
    }

    /// Declares an enum, returning its id.
    pub fn add_enum(&mut self, name: &str, variants: Vec<VariantDecl>) -> NamedId {
        self.entries.push(Decl {
            name: name.to_string(),
            fields: Vec::new(),
            variants,
            is_enum: true,
        });
        NamedId(self.entries.len() - 1)
    }

    pub fn get(&self, id: NamedId) -> &Decl {
        &self.entries[id.0]
    }

    pub fn get_mut(&mut self, id: NamedId) -> &mut Decl {
        &mut self.entries[id.0]
    }

    pub fn find(&self, name: &str) -> Option<NamedId> {
        self.entries
            .iter()
            .position(|d| d.name == name)
            .map(NamedId)
    }

    pub fn field_type(&self, id: NamedId, field: &str) -> Option<&Type> {
        self.get(id)
            .fields
            .iter()
            .find(|(n, _)| n == field)
            .map(|(_, t)| t)
    }

    pub fn variant(&self, id: NamedId, variant: &str) -> Option<&VariantDecl> {
        self.get(id).variants.iter().find(|v| v.name == variant)
    }
}

/// How two types may be combined, which is `SPEC.md` section 6.1 rule 4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arith {
    /// `Int` with `Int`.
    Int,
    /// `Float` with `Float`. An `Int` may be used where a `Float` is expected
    /// only through an `as` cast.
    Float,
    /// `Str` with `Str`, which is concatenation.
    Str,
}

/// Whether an operator is defined for a pair of types, and how.
///
/// Rule 4 is enforced here rather than by coercing: an `Int` and a `Float` do
/// not mix, and the only way between them is a cast.
pub fn arith_of(lhs: &Type, rhs: &Type) -> Option<Arith> {
    match (lhs, rhs) {
        (Type::Int, Type::Int) => Some(Arith::Int),
        (Type::Float, Type::Float) => Some(Arith::Float),
        (Type::Str, Type::Str) => Some(Arith::Str),
        _ => None,
    }
}

/// Whether `<`, `<=`, `>` and `>=` are defined for a pair of types.
pub fn ordered(lhs: &Type, rhs: &Type) -> bool {
    matches!(
        (lhs, rhs),
        (Type::Int, Type::Int)
            | (Type::Float, Type::Float)
            | (Type::Str, Type::Str)
            | (Type::Char, Type::Char)
    )
}

/// Whether a type can be constructed from a literal of the given kind. Used by
/// the `as` cast rules.
pub fn is_numeric(t: &Type) -> bool {
    matches!(t, Type::Int | Type::Float)
}

/// What a cast is allowed to do.
///
/// `SPEC.md` section 6.3 says a cast between unrelated types is checked at run
/// time, and this is the list of pairs that are worth a run time check. A cast
/// that is not in this table and not a widening is refused at compile time,
/// because it can never succeed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastKind {
    /// `Int as Float`, which always succeeds.
    Widen,
    /// `Float as Int`, which truncates.
    Narrow,
    /// Between two named types, checked at run time.
    Named,
}

impl CastKind {
    pub fn of(from: &Type, to: &Type) -> Option<CastKind> {
        use Type::*;
        match (from, to) {
            (Int, Float) => Some(CastKind::Widen),
            (Float, Int) => Some(CastKind::Narrow),
            (Int, Int) | (Float, Float) | (Bool, Bool) | (Str, Str) | (Char, Char) => {
                Some(CastKind::Widen)
            }
            (Named(_), Named(_)) => {
                // Only between two enums is a run time check meaningful, since
                // an enum value carries which variant it is.
                Some(CastKind::Named)
            }
            _ => None,
        }
    }
}
