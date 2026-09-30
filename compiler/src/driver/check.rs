//! The frontend pipeline: lex, parse, resolve, and type check.

use crate::diagnostic::Diagnostic;
use crate::hir;
use crate::mir::{lower, ownership};
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
/// drop insertion exists. Groundwork tests use it; no
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
    let (mut package, mut diagnostics) =
        checker::check(&parsed.file, resolution, file.text(), allow_move_types);
    if let Some(checked) = &package
        && allow_move_types
    {
        let program = lower::lower(checked);
        diagnostics.extend(ownership::check(&program));
    }
    diagnostics.sort_by_key(|d| (d.span().start(), d.span().end()));
    if !diagnostics.is_empty() {
        package = None;
    }
    Checked {
        package,
        diagnostics,
    }
}
