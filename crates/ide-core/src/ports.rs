//! The traits feature crates implement and `ide-ipc` consumes.
//!
//! The composition root (`apps/desktop/src-tauri`) builds the implementations
//! and hands them to `ide-ipc` as `Arc<dyn …>`, so no feature crate depends on
//! another and `ide-ipc` depends on none of them.

use std::path::Path;

use crate::IdeError;
use crate::fs::{DirListing, FileStamp, ReadLimits, ReadOutcome};
use crate::git::{DiffSide, DiffTexts, GitBranch, GitState};
use crate::path::RelPath;
use crate::search::{CancelFlag, FileList, TextQuery, TextSearchOutcome};
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

/// Searching a workspace. Both walks skip ignored files and the `.git` folder,
/// never follow symlinks, and stop early once `cancel` is set.
pub trait WorkspaceSearch: Send + Sync {
    /// The lines that match `query`, in every file it covers.
    fn search_text(
        &self,
        root: &WorkspaceRoot,
        query: &TextQuery,
        cancel: &CancelFlag,
    ) -> Result<TextSearchOutcome, IdeError>;

    /// Every file, for quick open.
    fn list_files(&self, root: &WorkspaceRoot, cancel: &CancelFlag) -> Result<FileList, IdeError>;
}

/// Git for a workspace. Paths are workspace-relative; an operation that git
/// refuses fails with [`IdeError::Git`] and git's own words.
pub trait GitClient: Send + Sync {
    /// Whether git can be used here, and if so the branch and what changed.
    fn state(&self, root: &WorkspaceRoot) -> Result<GitState, IdeError>;

    /// The two versions of `path` to compare, each at most `limit` bytes.
    fn diff_texts(
        &self,
        root: &WorkspaceRoot,
        path: &RelPath,
        side: DiffSide,
        limit: u64,
    ) -> Result<DiffTexts, IdeError>;

    /// Puts the files' changes (deletions included) in the index.
    fn stage(&self, root: &WorkspaceRoot, paths: &[RelPath]) -> Result<(), IdeError>;

    /// Takes the files' changes out of the index, keeping them in the working tree.
    fn unstage(&self, root: &WorkspaceRoot, paths: &[RelPath]) -> Result<(), IdeError>;

    /// Puts the files in the working tree back as they are in the index.
    /// Changes that weren't staged are lost; untracked files are left alone.
    fn discard(&self, root: &WorkspaceRoot, paths: &[RelPath]) -> Result<(), IdeError>;

    /// Commits what's staged, running the repository's hooks.
    fn commit(&self, root: &WorkspaceRoot, message: &str) -> Result<(), IdeError>;

    /// The local branches.
    fn branches(&self, root: &WorkspaceRoot) -> Result<Vec<GitBranch>, IdeError>;

    /// Switches to branch `name`, made from the current commit when `create`.
    fn switch_branch(&self, root: &WorkspaceRoot, name: &str, create: bool)
    -> Result<(), IdeError>;

    /// Makes the workspace folder a new repository.
    fn init(&self, root: &WorkspaceRoot) -> Result<(), IdeError>;
}
