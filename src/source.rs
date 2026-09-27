//! UTF-8 source files, file IDs, and byte spans.

use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_MAP_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FileId {
    map: u64,
    index: u32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Span {
    file: FileId,
    start: u32,
    end: u32,
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

#[derive(Debug)]
pub enum SourceError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    InvalidUtf8 {
        path: PathBuf,
        valid_up_to: usize,
    },
    TooLarge {
        path: PathBuf,
    },
    TooManyFiles,
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::InvalidUtf8 { path, valid_up_to } => {
                write!(f, "{}: invalid UTF-8 at byte {valid_up_to}", path.display())
            }
            Self::TooLarge { path } => write!(f, "{}: source exceeds 4 GiB", path.display()),
            Self::TooManyFiles => write!(f, "too many source files"),
        }
    }
}

impl Error for SourceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct SourceFile {
    id: FileId,
    path: PathBuf,
    text: String,
    line_starts: Vec<u32>,
}

impl SourceFile {
    pub fn id(&self) -> FileId {
        self.id
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn len(&self) -> u32 {
        self.text.len() as u32
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Returns the line without its line terminator.
    pub fn line(&self, zero_based: usize) -> Option<&str> {
        let start = *self.line_starts.get(zero_based)? as usize;
        let end = self
            .line_starts
            .get(zero_based + 1)
            .copied()
            .map_or(self.text.len(), |next| next as usize);
        let line = &self.text[start..end];
        Some(
            line.strip_suffix("\r\n")
                .or_else(|| line.strip_suffix('\n'))
                .unwrap_or(line),
        )
    }

    pub fn location(&self, offset: u32) -> Option<LineColumn> {
        let byte = offset as usize;
        if byte > self.text.len() || !self.text.is_char_boundary(byte) {
            return None;
        }
        let line_index = self.line_starts.partition_point(|&start| start <= offset) - 1;
        let line_start = self.line_starts[line_index] as usize;
        Some(LineColumn {
            line: line_index + 1,
            column: self.text[line_start..byte].chars().count() + 1,
        })
    }

    /// Returns a span if both ends are ordered UTF-8 boundaries in this file.
    pub fn span(&self, start: u32, end: u32) -> Option<Span> {
        let span = Span {
            file: self.id,
            start,
            end,
        };
        self.contains(span).then_some(span)
    }

    fn contains(&self, span: Span) -> bool {
        span.file == self.id
            && span.start <= span.end
            && span.end <= self.len()
            && self.text.is_char_boundary(span.start as usize)
            && self.text.is_char_boundary(span.end as usize)
    }
}

#[derive(Debug)]
pub struct SourceMap {
    map_id: u64,
    files: Vec<SourceFile>,
}

impl Default for SourceMap {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceMap {
    pub fn new() -> Self {
        let map_id = NEXT_MAP_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("source manager ID space exhausted");
        Self {
            map_id,
            files: Vec::new(),
        }
    }

    pub fn add(&mut self, path: impl Into<PathBuf>, text: String) -> Result<FileId, SourceError> {
        let path = path.into();
        if text.len() > u32::MAX as usize {
            return Err(SourceError::TooLarge { path });
        }
        let index = u32::try_from(self.files.len()).map_err(|_| SourceError::TooManyFiles)?;
        let id = FileId {
            map: self.map_id,
            index,
        };
        let mut line_starts = vec![0];
        for (offset, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push((offset + 1) as u32);
            }
        }
        self.files.push(SourceFile {
            id,
            path,
            text,
            line_starts,
        });
        Ok(id)
    }

    pub fn load(&mut self, path: impl AsRef<Path>) -> Result<FileId, SourceError> {
        let path = path.as_ref();
        let bytes = fs::read(path).map_err(|source| SourceError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if bytes.len() > u32::MAX as usize {
            return Err(SourceError::TooLarge {
                path: path.to_path_buf(),
            });
        }
        let text = String::from_utf8(bytes).map_err(|error| SourceError::InvalidUtf8 {
            path: path.to_path_buf(),
            valid_up_to: error.utf8_error().valid_up_to(),
        })?;
        self.add(path, text)
    }

    pub fn file(&self, id: FileId) -> Option<&SourceFile> {
        if id.map != self.map_id {
            return None;
        }
        self.files.get(id.index as usize)
    }

    pub fn span(&self, file: FileId, start: u32, end: u32) -> Option<Span> {
        self.file(file)?.span(start, end)
    }

    pub fn slice(&self, span: Span) -> Option<&str> {
        let file = self.file(span.file)?;
        file.contains(span)
            .then(|| &file.text[span.start as usize..span.end as usize])
    }
}
