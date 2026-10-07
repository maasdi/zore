use crate::ast;

/// Packages arrive dependencies first, and the entry package last.
pub struct PackageUnit<'a> {
    /// The import path, or `main` for the entry package.
    pub path: String,
    pub name: String,
    pub files: Vec<FileUnit<'a>>,
}

impl PackageUnit<'_> {
    /// Where the package clause is, for diagnostics about the package as a whole.
    pub fn clause(&self) -> Option<crate::source::Span> {
        self.files
            .first()?
            .file
            .package
            .as_ref()
            .map(|name| name.span)
    }
}

pub struct FileUnit<'a> {
    pub file: &'a ast::File,
    /// Aligned with `file.imports`.
    pub imports: Vec<ImportBinding>,
}

pub struct ImportBinding {
    /// Index into the packages being resolved.
    pub package: usize,
    /// The last segment of the import path.
    pub name: String,
}

pub struct PackageInfo {
    pub path: String,
    pub name: String,
    pub clause: Option<crate::source::Span>,
}
