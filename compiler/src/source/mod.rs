mod source_file;
mod source_map;
mod span;

pub use source_file::{SourceError, SourceFile};
pub use source_map::SourceMap;
pub use span::{FileId, LineColumn, Span};

/// Where the text behind a span can be read.
pub trait Sources {
    fn slice(&self, span: Span) -> Option<&str>;

    /// The file path and position of a span's start.
    fn locate(&self, span: Span) -> Option<(String, LineColumn)>;
}

/// Reads from `first`, then from `second`.
pub struct Layered<'a>(pub &'a dyn Sources, pub &'a dyn Sources);

impl Sources for Layered<'_> {
    fn slice(&self, span: Span) -> Option<&str> {
        self.0.slice(span).or_else(|| self.1.slice(span))
    }

    fn locate(&self, span: Span) -> Option<(String, LineColumn)> {
        self.0.locate(span).or_else(|| self.1.locate(span))
    }
}

impl Sources for SourceMap {
    fn slice(&self, span: Span) -> Option<&str> {
        SourceMap::slice(self, span)
    }

    fn locate(&self, span: Span) -> Option<(String, LineColumn)> {
        let file = self.file(span.file())?;
        Some((
            file.path().display().to_string(),
            file.location(span.start())?,
        ))
    }
}

impl Sources for SourceFile {
    fn slice(&self, span: Span) -> Option<&str> {
        self.contains(span)
            .then(|| &self.text()[span.start() as usize..span.end() as usize])
    }

    fn locate(&self, span: Span) -> Option<(String, LineColumn)> {
        if !self.contains(span) {
            return None;
        }
        Some((
            self.path().display().to_string(),
            self.location(span.start())?,
        ))
    }
}
