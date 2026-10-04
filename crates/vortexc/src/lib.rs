//! The Vortex compiler.
//!
//! Stage 1 contains the lexer. Later stages add the parser, the AST, the type
//! checker and the virtual machine, in the order set out in `ROADMAP.md`.

pub mod lexer;
pub mod span;
pub mod token;

pub use lexer::{lex, LexError};
pub use span::Pos;
pub use token::{Tok, Token};
