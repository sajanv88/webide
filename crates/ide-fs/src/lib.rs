//! Reads, writes and lists directories inside a workspace root.
//!
//! Every operation runs the same guard before any IO on the target:
//!
//! 1. The path is already a [`RelPath`]: relative, no `..`, valid names
//!    (checked in `ide-core`, without IO).
//! 2. The root is canonicalised again, and so is the target (following every
//!    symlink and junction). The target must be inside the canonical root,
//!    compared component by component, or the call fails with
//!    [`IdeError::OutsideWorkspace`].
//!
//! Known limit: between the check and the open, a process that can already
//! write inside the workspace could swap a folder for a symlink. Closing that
//! needs handle-relative IO (`openat`), which std doesn't offer.

mod guard;
mod write;

use std::fs;
use std::time::UNIX_EPOCH;

use ide_core::fs::{
    DirListing, FileNode, FileStamp, MAX_DIR_ENTRIES, NodeKind, ReadLimits, ReadOnlyReason,
    ReadOutcome, TEMP_FILE_PREFIX, detect_line_ending, looks_binary, sort_nodes,
};
use ide_core::{IdeError, RelPath, WorkspaceFs, WorkspaceRoot};

/// [`WorkspaceFs`] on the local disk.
#[derive(Debug, Default, Clone, Copy)]
pub struct LocalFs;

impl LocalFs {
    pub fn new() -> Self {
        Self
    }
}

impl WorkspaceFs for LocalFs {
    fn list_dir(&self, root: &WorkspaceRoot, dir: &RelPath) -> Result<DirListing, IdeError> {
        let target = guard::resolve_existing(root, dir)?;
        if !fs::metadata(&target)?.is_dir() {
            return Err(IdeError::NotADirectory);
        }

        let mut entries = Vec::new();
        let mut skipped = 0u32;
        for entry in fs::read_dir(&target)? {
            let Ok(entry) = entry else {
                skipped += 1;
                continue;
            };
            let Ok(name) = entry.file_name().into_string() else {
                skipped += 1;
                continue;
            };
            if name.starts_with(TEMP_FILE_PREFIX) {
                continue;
            }
            let Ok(path) = dir.join(&name) else {
                skipped += 1;
                continue;
            };
            let Ok(file_type) = entry.file_type() else {
                skipped += 1;
                continue;
            };
            let is_symlink = file_type.is_symlink();
            // For a symlink, describe what it points at (or Other when broken).
            let metadata = if is_symlink {
                fs::metadata(entry.path())
            } else {
                entry.metadata()
            };
            let (kind, size) = match metadata {
                Ok(meta) if meta.is_dir() => (NodeKind::Directory, None),
                Ok(meta) if meta.is_file() => (NodeKind::File, Some(meta.len())),
                _ => (NodeKind::Other, None),
            };
            entries.push(FileNode {
                name,
                path,
                kind,
                is_symlink,
                size,
            });
        }

        sort_nodes(&mut entries);
        let truncated = entries.len() > MAX_DIR_ENTRIES;
        entries.truncate(MAX_DIR_ENTRIES);
        Ok(DirListing {
            path: dir.clone(),
            entries,
            truncated,
            skipped,
        })
    }

    fn read_file(
        &self,
        root: &WorkspaceRoot,
        file: &RelPath,
        limits: ReadLimits,
    ) -> Result<ReadOutcome, IdeError> {
        let target = guard::resolve_existing(root, file)?;
        let metadata = fs::metadata(&target)?;
        if !metadata.is_file() {
            return Err(IdeError::NotAFile);
        }
        let stamp = stamp_of(&metadata);
        if stamp.size > limits.refuse_above {
            return Ok(ReadOutcome::TooLarge {
                stamp,
                limit: limits.refuse_above,
            });
        }

        let bytes = fs::read(&target)?;
        if looks_binary(&bytes) {
            return Ok(ReadOutcome::Binary { stamp });
        }
        let Ok(text) = String::from_utf8(bytes) else {
            return Ok(ReadOutcome::NotUtf8 { stamp });
        };

        let read_only = if stamp.size > limits.read_only_above {
            Some(ReadOnlyReason::TooLargeToEdit)
        } else if metadata.permissions().readonly() {
            Some(ReadOnlyReason::ReadOnlyOnDisk)
        } else {
            None
        };
        let line_ending = detect_line_ending(&text);
        Ok(ReadOutcome::Text {
            text,
            stamp,
            line_ending,
            read_only,
        })
    }

    fn write_file(
        &self,
        root: &WorkspaceRoot,
        file: &RelPath,
        text: &str,
    ) -> Result<FileStamp, IdeError> {
        let target = guard::resolve_for_write(root, file)?;
        write::atomic(&target, text.as_bytes())?;
        Ok(stamp_of(&fs::metadata(&target)?))
    }
}

fn stamp_of(metadata: &fs::Metadata) -> FileStamp {
    let modified_ms = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX));
    FileStamp {
        size: metadata.len(),
        modified_ms,
    }
}

#[cfg(test)]
mod tests;
