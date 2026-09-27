//! Frontend pipeline for `zore check`: lex, parse, resolve, and type check.

use crate::diagnostic::Diagnostic;
use crate::hir;
use crate::parser::parse;
use crate::resolve::resolve;
use crate::source::SourceFile;
use crate::typeck;

#[derive(Debug)]
pub struct Checked {
    /// Present only when every stage accepted the file.
    pub package: Option<hir::Package>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Check one file as a complete package. Semantic stages run only on a
/// syntactically valid file, so recovery artifacts cannot cause cascades.
pub fn check_file(file: &SourceFile) -> Checked {
    let parsed = parse(file);
    if !parsed.diagnostics.is_empty() {
        return Checked {
            package: None,
            diagnostics: parsed.diagnostics,
        };
    }
    let resolution = resolve(&parsed.file);
    let (package, mut diagnostics) = typeck::check(&parsed.file, resolution, file.text());
    // Report in source order rather than stage order.
    diagnostics.sort_by_key(|d| (d.span().start(), d.span().end()));
    Checked {
        package,
        diagnostics,
    }
}
