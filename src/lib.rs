//! The Zore bootstrap compiler: frontend, MIR, and LLVM IR backend.

pub mod ast;
pub mod bignum;
pub mod build;
pub mod check;
pub mod codegen;
pub mod constant;
pub mod diagnostic;
pub mod hir;
pub mod lexer;
pub mod lower;
pub mod mir;
pub mod parser;
pub mod resolve;
pub mod source;
pub mod token;
pub mod typeck;
pub mod types;
