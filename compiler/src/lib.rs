//! The Zore bootstrap compiler.

pub mod ast;
pub mod codegen;
pub mod diagnostic;
pub mod driver;
pub mod hir;
pub mod lexer;
pub mod mir;
pub mod parser;
pub mod resolve;
pub mod source;
pub mod types;

// Preserve the existing library paths for compiler clients.
pub use driver::{build, check};
pub use lexer::token;
pub use mir::lower;
pub use types::{bignum, checker as typeck, constant};
