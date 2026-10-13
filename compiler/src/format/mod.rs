mod layout;
mod writer;

use crate::diagnostic::{Diagnostic, Severity};
use crate::lexer::{Separator, TokenKind, lex};
use crate::parser::parse_standard;
use crate::source::{SourceFile, SourceMap};

pub fn format_file(file: &SourceFile) -> Result<String, Vec<Diagnostic>> {
    let parsed = parse_standard(file);
    if !parsed.diagnostics.is_empty() {
        return Err(parsed.diagnostics);
    }
    let lexed = lex(file);
    let layout = layout::Layout::new(file, &lexed.tokens, &parsed.file);
    let formatted = writer::write(file, &lexed.tokens, &layout);
    let mut sources = SourceMap::new();
    let id = sources
        .add(file.path(), formatted.clone())
        .map_err(|error| {
            vec![Diagnostic::new(
                Severity::Error,
                error.to_string(),
                file.span(0, 0).expect("start of source"),
            )]
        })?;
    let output = sources.file(id).expect("formatted source exists");
    let output_tokens = lex(output);
    if !output_tokens.diagnostics.is_empty()
        || !parse_standard(output).diagnostics.is_empty()
        || signature(file, &lexed.tokens) != signature(output, &output_tokens.tokens)
    {
        return Err(vec![Diagnostic::new(
            Severity::Error,
            "formatting would change the source tokens; the file was not modified",
            file.span(0, 0).expect("start of source"),
        )]);
    }
    Ok(formatted)
}

fn signature<'a>(file: &'a SourceFile, tokens: &[crate::lexer::Token]) -> Vec<&'a str> {
    tokens
        .iter()
        .enumerate()
        .filter_map(|(index, token)| match token.kind {
            TokenKind::Eof => None,
            TokenKind::Semicolon(Separator::Newline | Separator::Eof)
                if matches!(
                    tokens.get(index + 1).map(|t| &t.kind),
                    Some(TokenKind::Punct(crate::lexer::Punct::RBrace) | TokenKind::Eof)
                ) =>
            {
                None
            }
            TokenKind::Semicolon(_) => Some(";"),
            _ => Some(&file.text()[token.span.start() as usize..token.span.end() as usize]),
        })
        .collect()
}
