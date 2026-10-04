//! The Vortex compiler.
//!
//! Stage 2 contains the lexer, the parser, the AST, the lowering pass and the
//! tree interpreter. The type checker and the virtual machine follow, in the
//! order set out in `ROADMAP.md`.

pub mod ast;
pub mod interp;
pub mod ir;
pub mod lexer;
pub mod lower;
pub mod parser;
pub mod span;
pub mod token;
pub mod value;

pub use lexer::{lex, LexError};
pub use lower::{lower, LowerError};
pub use parser::{parse, ParseError};
pub use span::Pos;
pub use token::{Tok, Token};
pub use value::{RuntimeError, Value};

/// Parses, lowers and runs a program, writing whatever it prints to `out`.
///
/// This is the whole pipeline. A failure in any stage stops the program before
/// it runs, which is the point of lowering before execution.
pub fn run_source(src: &str, out: &mut dyn std::io::Write) -> Result<Value, Error> {
    let program = parse(src).map_err(Error::Parse)?;
    let lowered = lower(&program).map_err(Error::Lower)?;
    let value = interp::run(&lowered, out).map_err(Error::Runtime)?;
    Ok(value)
}

/// A failure in any stage of the pipeline.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    Parse(ParseError),
    Lower(LowerError),
    Runtime(RuntimeError),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Parse(e) => write!(f, "{}", e),
            Error::Lower(e) => write!(f, "{}", e),
            Error::Runtime(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for Error {}
