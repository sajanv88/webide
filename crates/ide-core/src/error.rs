use std::io;

use crate::path::PathError;

/// Every failure a domain operation can report.
///
/// `ide-ipc` maps each variant to a stable error code for the frontend, with an
/// exhaustive match, so adding a variant is a compile error there until mapped.
#[derive(Debug, thiserror::Error)]
pub enum IdeError {
    #[error("invalid path: {0}")]
    InvalidPath(#[from] PathError),
    #[error("the path is outside the workspace")]
    OutsideWorkspace,
    #[error("not found")]
    NotFound,
    #[error("not a directory")]
    NotADirectory,
    #[error("not a file")]
    NotAFile,
    #[error("permission denied")]
    PermissionDenied,
    #[error("no workspace is open")]
    NoWorkspace,
    #[error("the workspace changed since this request was made")]
    WorkspaceChanged,
    #[error("that folder isn't in the recent list: open it with the folder picker")]
    NotARecentWorkspace,
    #[error("invalid settings: {0}")]
    InvalidSettings(String),
    #[error("invalid search: {0}")]
    InvalidQuery(String),
    /// A value the operation can't take (a terminal size, too many terminals…).
    #[error("{0}")]
    InvalidArgument(String),
    /// Git refused or failed; git's own words.
    #[error("{0}")]
    Git(String),
    #[error("{0}")]
    Io(String),
}

impl IdeError {
    /// Maps an IO error to the domain error that says what went wrong.
    /// (Pure: it only looks at the error value.)
    pub fn from_io(error: &io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::NotFound => Self::NotFound,
            io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem => {
                Self::PermissionDenied
            }
            io::ErrorKind::NotADirectory => Self::NotADirectory,
            io::ErrorKind::IsADirectory => Self::NotAFile,
            _ => Self::Io(error.to_string()),
        }
    }
}

impl From<io::Error> for IdeError {
    fn from(error: io::Error) -> Self {
        Self::from_io(&error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_io_kinds() {
        let cases = [
            (io::ErrorKind::NotFound, "NotFound"),
            (io::ErrorKind::PermissionDenied, "PermissionDenied"),
            (io::ErrorKind::ReadOnlyFilesystem, "PermissionDenied"),
            (io::ErrorKind::NotADirectory, "NotADirectory"),
            (io::ErrorKind::IsADirectory, "NotAFile"),
            (io::ErrorKind::Other, "Io"),
        ];
        for (kind, expected) in cases {
            let mapped = IdeError::from(io::Error::from(kind));
            assert!(
                format!("{mapped:?}").starts_with(expected),
                "{kind:?} → {mapped:?}"
            );
        }
    }
}
