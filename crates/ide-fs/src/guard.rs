//! The containment guard: canonicalise, then check the target is inside the root.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ide_core::{IdeError, RelPath, WorkspaceRoot};

/// The canonical root, resolved again on every call.
fn canonical_root(root: &WorkspaceRoot) -> Result<PathBuf, IdeError> {
    let path = fs::canonicalize(root.path())?;
    if !fs::metadata(&path)?.is_dir() {
        return Err(IdeError::NotADirectory);
    }
    Ok(path)
}

/// `candidate` must be canonical. `Path::starts_with` compares whole components,
/// so `/ws-evil` is not inside `/ws`.
fn ensure_inside(root: &Path, candidate: &Path) -> Result<(), IdeError> {
    if candidate.starts_with(root) {
        Ok(())
    } else {
        Err(IdeError::OutsideWorkspace)
    }
}

/// An existing file or folder, with every symlink followed, checked to be inside.
pub(crate) fn resolve_existing(root: &WorkspaceRoot, rel: &RelPath) -> Result<PathBuf, IdeError> {
    let root = canonical_root(root)?;
    let target = fs::canonicalize(rel.to_native(&root))?;
    ensure_inside(&root, &target)?;
    Ok(target)
}

/// Where a write to `rel` should go: the existing file (symlinks followed and
/// checked), or a new name inside an existing folder that is itself inside.
pub(crate) fn resolve_for_write(root: &WorkspaceRoot, rel: &RelPath) -> Result<PathBuf, IdeError> {
    let (Some(parent), Some(name)) = (rel.parent(), rel.file_name()) else {
        return Err(IdeError::NotAFile);
    };
    let parent = resolve_existing(root, &parent)?;
    if !fs::metadata(&parent)?.is_dir() {
        return Err(IdeError::NotADirectory);
    }
    let candidate = parent.join(name);
    match fs::symlink_metadata(&candidate) {
        // A new file. Its folder is inside, and the name is one plain segment.
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(candidate),
        Err(error) => Err(IdeError::from_io(&error)),
        Ok(_) => {
            // Exists (perhaps as a symlink, perhaps a dangling one): follow and check.
            let root = canonical_root(root)?;
            let target = fs::canonicalize(&candidate)?;
            ensure_inside(&root, &target)?;
            let metadata = fs::metadata(&target)?;
            if !metadata.is_file() {
                return Err(IdeError::NotAFile);
            }
            if metadata.permissions().readonly() {
                return Err(IdeError::PermissionDenied);
            }
            Ok(target)
        }
    }
}
