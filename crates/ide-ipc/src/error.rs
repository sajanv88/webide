use ide_core::{IdeError, PathError};
use serde::Serialize;
use ts_rs::TS;

/// Stable error codes. The frontend switches on these, never on `message`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum IpcErrorCode {
    InvalidPath,
    OutsideWorkspace,
    NotFound,
    NotADirectory,
    NotAFile,
    PermissionDenied,
    NoWorkspace,
    WorkspaceChanged,
    NotARecentWorkspace,
    InvalidSettings,
    /// A search pattern or glob that doesn't parse. `message` says why.
    InvalidQuery,
    /// Git refused or failed. `message` is git's own words.
    Git,
    Io,
    /// A bug or an unexpected failure. `message` says what.
    Internal,
}

/// What every failing command rejects with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[error("{code:?}: {message}")]
pub struct IpcError {
    pub code: IpcErrorCode,
    /// For people and logs, in English.
    pub message: String,
}

impl IpcError {
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            code: IpcErrorCode::Internal,
            message: message.into(),
        }
    }
}

impl From<IdeError> for IpcError {
    fn from(error: IdeError) -> Self {
        // Exhaustive on purpose: a new IdeError variant must be given a code here.
        let code = match &error {
            IdeError::InvalidPath(_) => IpcErrorCode::InvalidPath,
            IdeError::OutsideWorkspace => IpcErrorCode::OutsideWorkspace,
            IdeError::NotFound => IpcErrorCode::NotFound,
            IdeError::NotADirectory => IpcErrorCode::NotADirectory,
            IdeError::NotAFile => IpcErrorCode::NotAFile,
            IdeError::PermissionDenied => IpcErrorCode::PermissionDenied,
            IdeError::NoWorkspace => IpcErrorCode::NoWorkspace,
            IdeError::WorkspaceChanged => IpcErrorCode::WorkspaceChanged,
            IdeError::NotARecentWorkspace => IpcErrorCode::NotARecentWorkspace,
            IdeError::InvalidSettings(_) => IpcErrorCode::InvalidSettings,
            IdeError::InvalidQuery(_) => IpcErrorCode::InvalidQuery,
            IdeError::Git(_) => IpcErrorCode::Git,
            IdeError::Io(_) => IpcErrorCode::Io,
        };
        Self {
            code,
            message: error.to_string(),
        }
    }
}

impl From<PathError> for IpcError {
    fn from(error: PathError) -> Self {
        IdeError::from(error).into()
    }
}
