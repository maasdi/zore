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
    /// Empty exactly when the file is valid.
    pub diagnostics: Vec<Diagnostic>,
    /// How many of the first `diagnostics` came from the lexer.
    pub lexer_diagnostics: usize,
}

pub fn parse(file: &SourceFile) -> Parsed {
    parse_with(file, false)
}

/// Accepts function declarations without bodies, which are native to the runtime.
pub fn parse_standard(file: &SourceFile) -> Parsed {
    parse_with(file, true)
}

fn parse_with(file: &SourceFile, native_functions: bool) -> Parsed {
    let lexed = lex(file);
    let lexer_diagnostics = lexed.diagnostics.len();
    let mut parser = Parser {
        file,
        tokens: lexed.tokens,
        pos: 0,
        last_token_end: 0,
        open_delimiters: 0,
        struct_literals_allowed: true,
        native_functions,
        diagnostics: lexed.diagnostics,
        token_edits: Vec::new(),
    };
    let ast = parser.file_ast();
    Parsed {
        file: ast,
        diagnostics: parser.diagnostics,
        lexer_diagnostics,
    }
}
