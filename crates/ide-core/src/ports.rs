//! The traits feature crates implement and `ide-ipc` consumes.
//!
//! The composition root (`apps/desktop/src-tauri`) builds the implementations
//! and hands them to `ide-ipc` as `Arc<dyn …>`, so no feature crate depends on
//! another and `ide-ipc` depends on none of them.

use std::path::Path;

use crate::IdeError;
use crate::fs::{DirListing, FileStamp, ReadLimits, ReadOutcome};
use crate::path::RelPath;
use crate::settings::{Settings, SettingsPatch};
use crate::watch::{WatchGuard, WatchSink};
use crate::workspace::{RecentWorkspace, WorkspaceRoot};

/// File operations inside a workspace.
///
/// Every implementation must, before any IO, canonicalise the target and reject
/// it with [`IdeError::OutsideWorkspace`] unless it's inside the canonical root.
pub trait WorkspaceFs: Send + Sync {
    /// One level of a directory.
    fn list_dir(&self, root: &WorkspaceRoot, dir: &RelPath) -> Result<DirListing, IdeError>;

    /// A file's text, or why it can't be shown as text.
    fn read_file(
        &self,
        root: &WorkspaceRoot,
        file: &RelPath,
        limits: ReadLimits,
    ) -> Result<ReadOutcome, IdeError>;

    /// Replaces a file's contents atomically, or creates it in an existing folder.
    fn write_file(
        &self,
        root: &WorkspaceRoot,
        file: &RelPath,
        text: &str,
    ) -> Result<FileStamp, IdeError>;
}

/// Opening workspaces, the recent list, and settings.
pub trait WorkspaceStore: Send + Sync {
    /// Canonicalises `path`, checks it's a folder, and records it as recent.
    fn open(&self, path: &Path) -> Result<WorkspaceRoot, IdeError>;

    /// Most recent first.
    fn recent(&self) -> Result<Vec<RecentWorkspace>, IdeError>;

    fn settings(&self) -> Result<Settings, IdeError>;

    /// Applies `patch`, saves, and returns the new settings.
    fn update_settings(&self, patch: &SettingsPatch) -> Result<Settings, IdeError>;
}

/// Watching a workspace for changes made by anyone: this app, other programs, git.
pub trait FsWatcher: Send + Sync {
    /// Starts watching `root` and everything under it, and returns at once.
    /// Events go to `sink`, on another thread, until the guard is dropped.
    /// A watch that can't start, or stops working, reports `WatchEvent::Failed`.
    fn watch(&self, root: &WorkspaceRoot, sink: WatchSink) -> WatchGuard;
}
