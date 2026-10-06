use crate::diagnostic::Diagnostic;
use crate::hir;
use crate::mir::{self, error_use};
use crate::ownership;
use crate::parser::parse;
use crate::resolve::resolve;
use crate::source::SourceFile;

#[derive(Debug)]
pub struct Checked {
    /// Present only when every stage accepted the file.
    pub package: Option<hir::Package>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn check_file(file: &SourceFile) -> Checked {
    let parsed = parse(file);
    if !parsed.diagnostics.is_empty() {
        return Checked {
            package: None,
            diagnostics: parsed.diagnostics,
        };
    }
    let resolution = resolve(&parsed.file);
    let (mut package, mut diagnostics) = hir::lower::check(&parsed.file, resolution, file.text());
    if let Some(checked) = &package {
        let program = mir::lower::lower(checked);
        diagnostics.extend(ownership::check(checked, &program));
        diagnostics.extend(error_use::check(checked, &program));
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
