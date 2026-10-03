//! One loaded UTF-8 source file and its line index.

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use super::span::{FileId, LineColumn, Span};

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
    pub(super) id: FileId,
    pub(super) path: PathBuf,
    pub(super) text: String,
    pub(super) line_starts: Vec<u32>,
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

    pub(super) fn contains(&self, span: Span) -> bool {
        span.file == self.id
            && span.start <= span.end
            && span.end <= self.len()
            && self.text.is_char_boundary(span.start as usize)
            && self.text.is_char_boundary(span.end as usize)
    }
}
