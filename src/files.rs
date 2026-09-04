//! Files to type over: loading, resuming where you left off, noticing changes.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::store::{FileProgress, Store};
use crate::text::file::{self, Chunk};

/// A file ready to type, with the chunk to start at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSession {
    /// Absolute path, the key for progress.
    pub path: String,
    pub content_hash: String,
    pub chunks: Vec<Chunk>,
    pub next_chunk: usize,
    /// Something worth telling the user, for example that the file changed.
    pub note: Option<&'static str>,
}

/// Which chunk to start at given what is stored, and why.
pub fn resume_point(
    progress: Option<&FileProgress>,
    content_hash: &str,
    chunks: usize,
) -> (usize, Option<&'static str>) {
    match progress {
        None => (0, None),
        Some(p) if p.content_hash != content_hash => {
            (0, Some("the file changed since last time, starting over"))
        }
        Some(p) if p.next_chunk >= chunks => (0, Some("typed to the end before, starting over")),
        Some(p) => (p.next_chunk, None),
    }
}

pub fn open(path: &Path, store: &Store) -> Result<FileSession> {
    let absolute = fs::canonicalize(path).with_context(|| format!("opening {}", path.display()))?;
    let raw =
        fs::read_to_string(&absolute).with_context(|| format!("reading {}", absolute.display()))?;
    let lines = file::normalise(&raw);
    let chunks = file::chunks(&lines);
    if chunks.is_empty() {
        bail!("{} is empty", path.display());
    }
    let content_hash = file::content_hash(&lines);
    let path = absolute.to_string_lossy().into_owned();
    let progress = store.file_progress(&path)?;
    let (next_chunk, note) = resume_point(progress.as_ref(), &content_hash, chunks.len());
    Ok(FileSession {
        path,
        content_hash,
        chunks,
        next_chunk,
        note,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress(hash: &str, next: usize, chunks: usize) -> FileProgress {
        FileProgress {
            path: "/f".into(),
            content_hash: hash.into(),
            next_chunk: next,
            chunks,
            updated_at: String::new(),
        }
    }

    #[test]
    fn resume_continues_restarts_or_starts_over() {
        assert_eq!(resume_point(None, "h", 5), (0, None));
        assert_eq!(resume_point(Some(&progress("h", 3, 5)), "h", 5), (3, None));
        assert_eq!(
            resume_point(Some(&progress("h", 5, 5)), "h", 5),
            (0, Some("typed to the end before, starting over"))
        );
        assert_eq!(
            resume_point(Some(&progress("old", 3, 5)), "h", 5),
            (0, Some("the file changed since last time, starting over"))
        );
    }

    #[test]
    fn open_loads_chunks_and_remembers_progress_by_absolute_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.rs");
        fs::write(&path, "fn main() {\n    println!(\"hi\");\n}\n").unwrap();
        let mut store = Store::open_in_memory().unwrap();
        let session = open(&path, &store).unwrap();
        assert!(Path::new(&session.path).is_absolute());
        assert_eq!(session.chunks.len(), 1);
        assert_eq!(session.next_chunk, 0);
        store
            .set_file_progress(&session.path, &session.content_hash, 1, 1)
            .unwrap();
        let again = open(&path, &store).unwrap();
        assert_eq!(again.next_chunk, 0);
        assert!(again.note.is_some());
        fs::write(dir.path().join("empty.txt"), "\n\n").unwrap();
        assert!(open(&dir.path().join("empty.txt"), &store).is_err());
        assert!(open(&dir.path().join("missing.txt"), &store).is_err());
    }
}
