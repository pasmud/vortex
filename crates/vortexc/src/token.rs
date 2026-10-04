//! Lexical tokens.
//!
//! The token set is defined in `SPEC.md` section 5. String and character
//! literals carry their **decoded** value, because escapes are resolved during
//! lexing.

/// A token kind.
///
/// Not `Copy`, because a string token owns its decoded value, and not `Eq`,
/// because a float literal is an `f64`.
#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    // Literals and names.
    Int(u64),
    Float(f64),
    /// A string literal, holding the decoded contents without the quotes.
    Str(String),
    /// A character literal, holding the decoded scalar value.
    Char(char),
    /// An identifier or keyword. Keywords are recognised by the parser, not by
    /// the lexer, so that the keyword list can grow without changing this file.
    Ident(&'static str),

    // Punctuation and operators.
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Assign,
    PlusAssign,
    MinusAssign,
    StarAssign,
    SlashAssign,
    PercentAssign,
    EqEq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    AndAnd,
    OrOr,
    /// A single `&`. Vortex v0.1 has no references, so this token exists only
    /// to give a precise diagnostic when someone writes one.
    Amp,
    /// A single `|`, used to separate patterns in one `match` arm.
    Pipe,
    Bang,
    Arrow,
    FatArrow,
    DotDot,
    DotDotEq,
    PathSep,
    Question,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Dot,
    Comma,
    Semicolon,
    Colon,

    /// End of file. Its position is just past the last character.
    Eof,
}

impl Tok {
    /// A human readable name used in diagnostics.
    pub fn describe(&self) -> String {
        match self {
            Tok::Int(v) => format!("integer `{}`", v),
            Tok::Float(v) => format!("float `{}`", v),
            Tok::Str(_) => "string literal".to_string(),
            Tok::Char(_) => "character literal".to_string(),
            Tok::Ident(name) => format!("identifier `{}`", name),
            Tok::Eof => "end of file".to_string(),
            other => format!("`{}`", other.spelling()),
        }
    }

    /// The source spelling of a fixed token.
    fn spelling(&self) -> &'static str {
        match self {
            Tok::Plus => "+",
            Tok::Minus => "-",
            Tok::Star => "*",
            Tok::Slash => "/",
            Tok::Percent => "%",
            Tok::Assign => "=",
            Tok::PlusAssign => "+=",
            Tok::MinusAssign => "-=",
            Tok::StarAssign => "*=",
            Tok::SlashAssign => "/=",
            Tok::PercentAssign => "%=",
            Tok::EqEq => "==",
            Tok::NotEq => "!=",
            Tok::Lt => "<",
            Tok::LtEq => "<=",
            Tok::Gt => ">",
            Tok::GtEq => ">=",
            Tok::AndAnd => "&&",
            Tok::OrOr => "||",
            Tok::Amp => "&",
            Tok::Pipe => "|",
            Tok::Bang => "!",
            Tok::Arrow => "->",
            Tok::FatArrow => "=>",
            Tok::DotDot => "..",
            Tok::DotDotEq => "..=",
            Tok::PathSep => "::",
            Tok::Question => "?",
            Tok::LParen => "(",
            Tok::RParen => ")",
            Tok::LBrace => "{",
            Tok::RBrace => "}",
            Tok::LBracket => "[",
            Tok::RBracket => "]",
            Tok::Dot => ".",
            Tok::Comma => ",",
            Tok::Semicolon => ";",
            Tok::Colon => ":",
            Tok::Int(_) | Tok::Float(_) | Tok::Str(_) | Tok::Char(_) | Tok::Ident(_) => "literal",
            Tok::Eof => "end of file",
        }
    }
}

/// A token together with the span it covers.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub start: crate::span::Pos,
    /// The position just past the last character of the token.
    pub end: crate::span::Pos,
}
