//! The `webide` command: what a terminal asked the app to open, and putting
//! the command on the PATH.

use std::path::PathBuf;

use crate::IdeError;

/// What `webide <path>` asked to open: a folder, or a file in its folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    /// The folder to open as the workspace: an absolute path.
    pub folder: PathBuf,
    /// A file in `folder` to open in it, when a file was named.
    pub file: Option<PathBuf>,
}

/// Where the `webide` command stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandLineStatus {
    /// `webide` in a terminal runs this app. `path` is where it was put;
    /// `hint` is what's left to do, if anything (open a new terminal…).
    Installed { path: String, hint: Option<String> },
    /// It can be put on the PATH.
    NotInstalled,
    /// This copy of the app can't put it there: why.
    Unavailable { reason: String },
}

/// Puts the `webide` command on the PATH, or takes it off.
pub trait CommandLine: Send + Sync {
    fn status(&self) -> CommandLineStatus;

    /// Puts it on the PATH (it may ask the system for permission).
    fn install(&self) -> Result<CommandLineStatus, IdeError>;

    /// Takes off what `install` put there, and nothing else.
    fn uninstall(&self) -> Result<CommandLineStatus, IdeError>;
}
