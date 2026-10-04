//! The Vortex compiler.
//!
//! The pipeline is lex, parse, lower, check, run. Stage 3 added the checker, so
//! a program that breaks a type rule is rejected before the interpreter sees
//! it, which is what `SPEC.md` section 6.1 rule 1 requires.

pub mod ast;
pub mod bytecode;
pub mod checker;
pub mod interp;
pub mod ir;
pub mod lexer;
pub mod lower;
pub mod parser;
pub mod span;
pub mod token;
pub mod types;
pub mod value;

pub use checker::{check, TypeError};
pub use lexer::{lex, LexError};
pub use lower::{lower, LowerError};
pub use parser::{parse, ParseError};
pub use span::Pos;
pub use token::{Tok, Token};
pub use value::{RuntimeError, Value};

/// Parses, lowers, checks and runs a program, writing whatever it prints to
/// `out`.
///
/// A failure in any stage stops the program before it runs. That is the point:
/// the checker runs before the interpreter, so an ill typed program never
/// executes, and neither does one with an undefined name or a wrong argument
/// count.
pub fn run_source(src: &str, out: &mut dyn std::io::Write) -> Result<Value, Error> {
    check_source(src).and_then(|(lowered, _)| {
        let value = interp::run(&lowered, out).map_err(Error::Runtime)?;
        Ok(value)
    })
}

/// Runs every stage except the interpreter, returning the lowered program.
///
/// This is what the benchmark and the tools use, and it is how a test shows
/// that a program fails the checker without running.
pub fn check_source(src: &str) -> Result<(ir::Program, types::Decls), Error> {
    let program = parse(src).map_err(Error::Parse)?;
    let lowered = lower(&program).map_err(Error::Lower)?;
    let decls = check(&lowered).map_err(Error::Type)?;
    Ok((lowered, decls))
}

/// A failure in any stage of the pipeline.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    Parse(ParseError),
    Lower(LowerError),
    Type(TypeError),
    Runtime(RuntimeError),
}

impl Error {
    /// The stage that failed, named as it appears in the pipeline.
    pub fn stage(&self) -> &'static str {
        match self {
            Error::Parse(_) => "parse",
            Error::Lower(_) => "lower",
            Error::Type(_) => "check",
            Error::Runtime(_) => "run",
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Parse(e) => write!(f, "{}", e),
            Error::Lower(e) => write!(f, "{}", e),
            Error::Type(e) => write!(f, "{}", e),
            Error::Runtime(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for Error {}
