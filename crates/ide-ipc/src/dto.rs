//! What crosses the IPC boundary. Exported to `packages/contracts` with ts-rs,
//! so the TypeScript types are never written by hand.
//!
//! Conventions: camelCase fields, `/`-separated workspace-relative paths,
//! sizes and times as plain numbers (u64 is exported as `number`; every value
//! here is far below 2^53).

use ide_core::fs::{
    DirListing, FileNode, FileStamp, LineEnding, NodeKind, ReadOnlyReason, ReadOutcome,
};
use ide_core::workspace::display_path;
use ide_core::{FsChange, RecentWorkspace, RelPath, Settings, SettingsPatch, Theme};
use serde::{Deserialize, Serialize};
use tauri::ipc::JavaScriptChannelId;
use ts_rs::TS;

// ---------- Outputs ----------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AppInfoDto {
    /// `productName` from tauri.conf.json.
    pub name: String,
    pub version: String,
}

/// The open workspace. `id` changes every time a workspace is opened; file
/// commands must send it back, so a request meant for one workspace can't act
/// on another.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDto {
    pub id: String,
    pub name: String,
    /// The folder, as people write it.
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RecentWorkspaceDto {
    pub name: String,
    /// The folder, as people write it. Pass it to `open_recent_workspace`.
    pub path: String,
    pub opened_at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum NodeKindDto {
    File,
    Directory,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FileNodeDto {
    pub name: String,
    /// Relative to the workspace root, `/`-separated.
    pub path: String,
    /// What it is, following a symlink.
    pub kind: NodeKindDto,
    pub is_symlink: bool,
    /// Bytes, for files.
    pub size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DirListingDto {
    pub path: String,
    /// Folders first, then by name.
    pub entries: Vec<FileNodeDto>,
    /// The folder has more entries than one listing returns.
    pub truncated: bool,
    /// Entries left out because their names can't be represented.
    pub skipped: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FileStampDto {
    pub size: u64,
    /// Milliseconds since the Unix epoch.
    pub modified_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum LineEndingDto {
    Lf,
    Crlf,
    Mixed,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ReadOnlyReasonDto {
    /// Larger than the "read only above" setting.
    TooLargeToEdit,
    /// The file system says it can't be written.
    ReadOnlyOnDisk,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TextFileDto {
    pub path: String,
    pub text: String,
    pub stamp: FileStampDto,
    pub line_ending: LineEndingDto,
    /// Set when the file must open read-only, and why.
    pub read_only: Option<ReadOnlyReasonDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TooLargeFileDto {
    pub path: String,
    pub stamp: FileStampDto,
    /// The "refuse above" setting it exceeded.
    pub limit: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UnreadableFileDto {
    pub path: String,
    pub stamp: FileStampDto,
}

/// A file's text, or why it can't be shown as text. Switch on `kind`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ReadFileDto {
    Text(TextFileDto),
    TooLarge(TooLargeFileDto),
    Binary(UnreadableFileDto),
    NotUtf8(UnreadableFileDto),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum ThemeDto {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    pub theme: ThemeDto,
    /// Files larger than this open read-only.
    pub read_only_above_bytes: u64,
    /// Files larger than this don't open.
    pub refuse_above_bytes: u64,
}

// ---------- Events: backend → webview ----------

/// One change on disk. `path` is the path as it is now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FileChangeDto {
    Created {
        path: String,
    },
    /// Its contents or its metadata changed.
    Modified {
        path: String,
    },
    Removed {
        path: String,
    },
    /// Moved within the workspace, when the system reports both names together;
    /// otherwise a move arrives as `removed` and `created`.
    Renamed {
        from: String,
        path: String,
    },
}

/// What the backend tells the webview, over the channel given to
/// `subscribe_events`. Switch on `kind`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BackendEventDto {
    /// Files of the workspace `workspaceId` changed on disk, by anyone. With
    /// `rescan`, `changes` is incomplete (or empty): reload everything shown.
    #[serde(rename_all = "camelCase")]
    FilesChanged {
        workspace_id: String,
        changes: Vec<FileChangeDto>,
        rescan: bool,
    },
    /// Watching `workspaceId` failed: outside changes may go unnoticed.
    #[serde(rename_all = "camelCase")]
    WatchFailed {
        workspace_id: String,
        message: String,
    },
    /// The window is closing while `set_close_guard` says there are unsaved
    /// changes. It stays open until the webview calls `close_window`.
    CloseRequested,
}

// ---------- Arguments ----------

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenRecentArgs {
    /// A `path` from `recent_workspaces`.
    pub path: String,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspacePathArgs {
    /// `id` of the open workspace.
    pub workspace_id: String,
    /// Relative to the workspace root, `/`-separated; `""` is the root.
    pub path: String,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriteFileArgs {
    pub workspace_id: String,
    pub path: String,
    pub text: String,
}

/// Only the fields present change.
#[derive(Debug, Clone, Copy, Default, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsPatchDto {
    #[serde(default)]
    #[ts(optional)]
    pub theme: Option<ThemeDto>,
    #[serde(default)]
    #[ts(optional)]
    pub read_only_above_bytes: Option<u64>,
    #[serde(default)]
    #[ts(optional)]
    pub refuse_above_bytes: Option<u64>,
}

/// Where backend events go: one channel per page, replaced when the page
/// subscribes again (after a reload).
#[derive(Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubscribeEventsArgs {
    /// A `Channel<BackendEventDto>` from `@tauri-apps/api/core`; only `@repo/ipc` makes one.
    #[ts(type = "unknown")]
    pub channel: JavaScriptChannelId,
}

impl std::fmt::Debug for SubscribeEventsArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubscribeEventsArgs")
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloseGuardArgs {
    /// Whether closing the window now would lose unsaved changes.
    pub unsaved: bool,
}

// ---------- Domain → DTO ----------

impl From<&RecentWorkspace> for RecentWorkspaceDto {
    fn from(recent: &RecentWorkspace) -> Self {
        Self {
            name: recent.name.clone(),
            path: display_path(&recent.path),
            opened_at_ms: recent.opened_at_ms,
        }
    }
}

impl From<NodeKind> for NodeKindDto {
    fn from(kind: NodeKind) -> Self {
        match kind {
            NodeKind::File => Self::File,
            NodeKind::Directory => Self::Directory,
            NodeKind::Other => Self::Other,
        }
    }
}

impl From<FileNode> for FileNodeDto {
    fn from(node: FileNode) -> Self {
        Self {
            name: node.name,
            path: node.path.as_str().to_owned(),
            kind: node.kind.into(),
            is_symlink: node.is_symlink,
            size: node.size,
        }
    }
}

impl From<DirListing> for DirListingDto {
    fn from(listing: DirListing) -> Self {
        Self {
            path: listing.path.as_str().to_owned(),
            entries: listing.entries.into_iter().map(Into::into).collect(),
            truncated: listing.truncated,
            skipped: listing.skipped,
        }
    }
}

impl From<FileStamp> for FileStampDto {
    fn from(stamp: FileStamp) -> Self {
        Self {
            size: stamp.size,
            modified_ms: stamp.modified_ms,
        }
    }
}

impl From<LineEnding> for LineEndingDto {
    fn from(ending: LineEnding) -> Self {
        match ending {
            LineEnding::Lf => Self::Lf,
            LineEnding::Crlf => Self::Crlf,
            LineEnding::Mixed => Self::Mixed,
            LineEnding::None => Self::None,
        }
    }
}

impl From<ReadOnlyReason> for ReadOnlyReasonDto {
    fn from(reason: ReadOnlyReason) -> Self {
        match reason {
            ReadOnlyReason::TooLargeToEdit => Self::TooLargeToEdit,
            ReadOnlyReason::ReadOnlyOnDisk => Self::ReadOnlyOnDisk,
        }
    }
}

impl ReadFileDto {
    pub fn from_outcome(path: &RelPath, outcome: ReadOutcome) -> Self {
        let path = path.as_str().to_owned();
        match outcome {
            ReadOutcome::Text {
                text,
                stamp,
                line_ending,
                read_only,
            } => Self::Text(TextFileDto {
                path,
                text,
                stamp: stamp.into(),
                line_ending: line_ending.into(),
                read_only: read_only.map(Into::into),
            }),
            ReadOutcome::TooLarge { stamp, limit } => Self::TooLarge(TooLargeFileDto {
                path,
                stamp: stamp.into(),
                limit,
            }),
            ReadOutcome::Binary { stamp } => Self::Binary(UnreadableFileDto {
                path,
                stamp: stamp.into(),
            }),
            ReadOutcome::NotUtf8 { stamp } => Self::NotUtf8(UnreadableFileDto {
                path,
                stamp: stamp.into(),
            }),
        }
    }
}

impl From<FsChange> for FileChangeDto {
    fn from(change: FsChange) -> Self {
        let text = |path: RelPath| path.as_str().to_owned();
        match change {
            FsChange::Created(path) => Self::Created { path: text(path) },
            FsChange::Modified(path) => Self::Modified { path: text(path) },
            FsChange::Removed(path) => Self::Removed { path: text(path) },
            FsChange::Renamed { from, to } => Self::Renamed {
                from: text(from),
                path: text(to),
            },
        }
    }
}

impl From<Theme> for ThemeDto {
    fn from(theme: Theme) -> Self {
        match theme {
            Theme::System => Self::System,
            Theme::Light => Self::Light,
            Theme::Dark => Self::Dark,
        }
    }
}

impl From<ThemeDto> for Theme {
    fn from(theme: ThemeDto) -> Self {
        match theme {
            ThemeDto::System => Self::System,
            ThemeDto::Light => Self::Light,
            ThemeDto::Dark => Self::Dark,
        }
    }
}

impl From<Settings> for SettingsDto {
    fn from(settings: Settings) -> Self {
        Self {
            theme: settings.theme.into(),
            read_only_above_bytes: settings.read_only_above_bytes,
            refuse_above_bytes: settings.refuse_above_bytes,
        }
    }
}

impl From<SettingsPatchDto> for SettingsPatch {
    fn from(patch: SettingsPatchDto) -> Self {
        Self {
            theme: patch.theme.map(Into::into),
            read_only_above_bytes: patch.read_only_above_bytes,
            refuse_above_bytes: patch.refuse_above_bytes,
        }
    }
}
