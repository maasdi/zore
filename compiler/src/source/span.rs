//! File identities, byte spans, and line/column locations.

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FileId {
    pub(super) map: u64,
    pub(super) index: u32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Span {
    pub(super) file: FileId,
    pub(super) start: u32,
    pub(super) end: u32,
}

impl Span {
    pub fn file(self) -> FileId {
        self.file
    }

    pub fn start(self) -> u32 {
        self.start
    }

    pub fn end(self) -> u32 {
        self.end
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LineColumn {
    /// One-based line number.
    pub line: usize,
    /// One-based Unicode scalar column.
    pub column: usize,
}
