use std::path::{Path, PathBuf};

use crate::IdeError;

/// How many recent workspaces are remembered.
pub const MAX_RECENT: usize = 10;

/// The open workspace: an absolute, canonical folder.
///
/// It's made by `WorkspaceStore::open`, which canonicalises the path first.
/// File operations canonicalise it again before use, so a root that moved or
/// became a symlink can't widen what they reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRoot {
    path: PathBuf,
    name: String,
}

impl WorkspaceRoot {
    /// `path` must be absolute and already canonical.
    pub fn from_canonical(path: PathBuf) -> Result<Self, IdeError> {
        if !path.is_absolute() {
            return Err(IdeError::Io(format!(
                "workspace root {} is not absolute",
                path.display()
            )));
        }
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| display_path(&path));
        Ok(Self { path, name })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The folder's name, for titles.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The path as people write it (see [`display_path`]).
    pub fn display_path(&self) -> String {
        display_path(&self.path)
    }
}

/// A workspace opened before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentWorkspace {
    /// Canonical path, as opened.
    pub path: PathBuf,
    pub name: String,
    pub opened_at_ms: u64,
}

/// A path as people write it: on Windows, canonical paths carry the `\\?\`
/// prefix (`\\?\C:\src`, `\\?\UNC\server\share`), which this removes.
pub fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        let bytes = rest.as_bytes();
        if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            return rest.to_owned();
        }
    }
    text.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_path_drops_the_verbatim_prefix() {
        let cases = [
            (r"\\?\C:\src\app", r"C:\src\app"),
            (r"\\?\UNC\server\share\app", r"\\server\share\app"),
            (r"\\?\Volume{1234}\app", r"\\?\Volume{1234}\app"),
            ("/home/me/app", "/home/me/app"),
        ];
        for (input, expected) in cases {
            assert_eq!(display_path(Path::new(input)), expected);
        }
    }

    #[test]
    fn root_needs_an_absolute_path() {
        assert!(WorkspaceRoot::from_canonical(PathBuf::from("relative")).is_err());
        let absolute = std::env::temp_dir();
        let root = WorkspaceRoot::from_canonical(absolute.clone()).unwrap();
        assert_eq!(root.path(), absolute);
        assert!(!root.name().is_empty());
    }
}
