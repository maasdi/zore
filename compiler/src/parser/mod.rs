//! Tokens to syntax tree, recovering from errors at statement and
//! declaration boundaries.

mod declaration;
mod expression;
#[allow(clippy::module_inception)]
mod parser;
mod statement;
mod type_syntax;

use crate::ast::File;
use crate::diagnostic::Diagnostic;
use crate::lexer::lex;
use crate::source::SourceFile;
use parser::Parser;

#[derive(Debug)]
pub struct Parsed {
    pub file: File,
    /// Lexical then syntax diagnostics; empty exactly when the file is valid.
    pub diagnostics: Vec<Diagnostic>,
}

pub fn parse(file: &SourceFile) -> Parsed {
    let lexed = lex(file);
    let mut parser = Parser {
        file,
        tokens: lexed.tokens,
        pos: 0,
        last_token_end: 0,
        open_delimiters: 0,
        struct_literals_allowed: true,
        diagnostics: lexed.diagnostics,
    };
    let ast = parser.file_ast();
    Parsed {
        file: ast,
        diagnostics: parser.diagnostics,
    }
}
