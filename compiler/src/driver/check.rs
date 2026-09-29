//! The frontend pipeline: lex, parse, resolve, and type check.

use crate::diagnostic::Diagnostic;
use crate::hir;
use crate::parser::parse;
use crate::resolve::resolve;
use crate::source::SourceFile;
use crate::types::checker;

#[derive(Debug)]
pub struct Checked {
    /// Present only when every stage accepted the file.
    pub package: Option<hir::Package>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Checks one file as a complete package.
pub fn check_file(file: &SourceFile) -> Checked {
    check_with(file, false)
}

/// Like `check_file`, but accepts Move types, which are otherwise rejected until
/// ownership analysis and drop insertion exist. Groundwork tests use it; no
/// command does.
pub fn check_file_allowing_move_types(file: &SourceFile) -> Checked {
    check_with(file, true)
}

fn check_with(file: &SourceFile, allow_move_types: bool) -> Checked {
    let parsed = parse(file);
    if !parsed.diagnostics.is_empty() {
        return Checked {
            package: None,
            diagnostics: parsed.diagnostics,
        };
    }
    let resolution = resolve(&parsed.file);
    let (package, mut diagnostics) =
        checker::check(&parsed.file, resolution, file.text(), allow_move_types);
    diagnostics.sort_by_key(|d| (d.span().start(), d.span().end()));
    Checked {
        package,
        diagnostics,
    }
}
