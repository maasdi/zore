//! The source manager: owns files and issues their IDs.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::source_file::{SourceError, SourceFile};
use super::span::{FileId, Span};

static NEXT_MAP_ID: AtomicU64 = AtomicU64::new(1);

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
