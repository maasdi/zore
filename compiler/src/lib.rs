pub mod ast;
pub mod codegen;
pub mod diagnostic;
pub mod driver;
pub mod dropck;
pub mod hir;
pub mod lexer;
pub mod mir;
pub mod ownership;
pub mod parser;
pub mod resolve;
pub mod source;
pub mod types;

pub use driver::{build, check};
