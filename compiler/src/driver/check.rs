use super::project::{Project, load_file};
use crate::diagnostic::Diagnostic;
use crate::hir;
use crate::mir;
use crate::ownership::{self, error_use};
use crate::resolve::resolve;
use crate::source::{Layered, SourceFile, Sources};

#[derive(Debug)]
pub struct Checked {
    /// Present only when every stage accepted the program.
    pub package: Option<hir::Package>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Checks one file as the whole entry package; only standard packages can be imported.
pub fn check_file(file: &SourceFile) -> Checked {
    match load_file(file) {
        Ok(project) => check_project(&project, file),
        Err(diagnostics) => Checked {
            package: None,
            diagnostics,
        },
    }
}

/// `sources` holds the text of the project's own files.
pub fn check_project(project: &Project, sources: &dyn Sources) -> Checked {
    let units = project.units();
    let resolution = resolve(&units);
    let layered = Layered(sources, project.std_sources());
    let (mut package, mut diagnostics) = hir::lower::check(resolution, &layered);
    if let Some(checked) = &package {
        let program = mir::lower::lower(checked);
        diagnostics.extend(ownership::check(checked, &program));
        diagnostics.extend(error_use::check(checked, &program));
    }
    diagnostics.sort_by_key(|d| (d.span().file(), d.span().start(), d.span().end()));
    if !diagnostics.is_empty() {
        package = None;
    }
    Checked {
        package,
        diagnostics,
    }
}
