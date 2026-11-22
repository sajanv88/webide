//! What crosses the IPC boundary. Exported to `packages/contracts` with ts-rs,
//! so the TypeScript types are never written by hand.
//!
//! Conventions: camelCase fields, `/`-separated workspace-relative paths,
//! sizes and times as plain numbers (u64 is exported as `number`; every value
//! here is far below 2^53).

use ide_core::fs::{
    DirListing, FileNode, FileStamp, LineEnding, NodeKind, ReadOnlyReason, ReadOutcome,
};
use ide_core::git::{
    DiffSide, DiffTexts, GitBranch, GitChange, GitFileStatus, GitHead, GitState, GitStatus,
};
use ide_core::language::{
    Completion, CompletionKind, Completions, Definition, HoverText, LanguageStatus, Problem,
    Severity, TextPosition, TextRange,
};
use ide_core::workspace::display_path;
use ide_core::{
    FileList, FileMatches, FsChange, LineMatch, RecentWorkspace, RelPath, Settings, SettingsPatch,
    Span, TerminalEvent, TextSearchOutcome, Theme,
};
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
    GitInternal,
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
    /// The workspace's language server started, is ready, or failed.
    #[serde(rename_all = "camelCase")]
    LanguageStatus {
        workspace_id: String,
        status: LanguageStatusDto,
    },
}

/// A span inside a search preview, in UTF-16 code units (JavaScript string indices).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SpanDto {
    pub start: u32,
    pub end: u32,
}

/// One line that matched a search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LineMatchDto {
    /// 1-based.
    pub line: u32,
    /// 1-based, in UTF-16 units of the whole line: where the first match starts.
    pub column: u32,
    /// The line, or the part around the first match when it's long.
    pub preview: String,
    /// Every match in `preview`.
    pub spans: Vec<SpanDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FileMatchesDto {
    pub path: String,
    pub lines: Vec<LineMatchDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TextSearchDto {
    pub files: Vec<FileMatchesDto>,
    /// It stopped at the match limit: there may be more.
    pub truncated: bool,
    /// A newer search replaced this one; its results are partial.
    pub cancelled: bool,
}

/// The workspace's files for quick open, ignored ones left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FileListDto {
    pub files: Vec<String>,
    /// It stopped at the file limit.
    pub truncated: bool,
}

/// What happened to a file on one side (the index, or the working tree).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum GitChangeDto {
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Untracked,
    Conflicted,
}

/// One changed file in the workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GitFileDto {
    pub path: String,
    /// Where it was renamed or copied from.
    pub from: Option<String>,
    /// Its change in the index: staged for the next commit.
    pub staged: Option<GitChangeDto>,
    /// Its change in the working tree that isn't staged.
    pub unstaged: Option<GitChangeDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GitHeadDto {
    /// `null` when the head is detached.
    pub branch: Option<String>,
    /// Abbreviated; `null` before the first commit.
    pub commit: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
}

/// Whether the workspace can use git, and if so its branch and changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum GitStateDto {
    /// No `git` program was found.
    Unavailable,
    NotARepository,
    #[serde(rename_all = "camelCase")]
    Repository {
        head: GitHeadDto,
        /// In path order.
        files: Vec<GitFileDto>,
        /// More files changed than are listed.
        truncated: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum DiffSideDto {
    /// The last commit and the index.
    Staged,
    /// The index and the working tree.
    Unstaged,
}

/// The two versions of a file to compare.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum GitDiffDto {
    /// Empty where the file doesn't exist on that side.
    #[serde(rename_all = "camelCase")]
    Text {
        original: String,
        modified: String,
    },
    Binary,
    #[serde(rename_all = "camelCase")]
    TooLarge {
        limit: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GitBranchDto {
    pub name: String,
    pub current: bool,
}

/// A terminal that was started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TerminalDto {
    pub id: u32,
    /// The shell's name, for its tab: `pwsh`, `zsh`.
    pub shell: String,
}

/// What a terminal sends on its channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TerminalEventDto {
    /// Text the shell wrote, with its escape sequences, for the terminal to draw.
    #[serde(rename_all = "camelCase")]
    Output { data: String },
    /// The shell ended. Nothing follows.
    #[serde(rename_all = "camelCase")]
    Exited { code: Option<u32> },
}

/// A place in a text: 0-based line, and column in UTF-16 units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TextPositionDto {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TextRangeDto {
    pub start: TextPositionDto,
    pub end: TextPositionDto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum SeverityDto {
    Error,
    Warning,
    Info,
    Hint,
}

/// A problem a language server found in a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProblemDto {
    pub range: TextRangeDto,
    pub severity: SeverityDto,
    pub message: String,
    pub source: Option<String>,
    pub code: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum CompletionKindDto {
    Text,
    Method,
    Function,
    Constructor,
    Field,
    Variable,
    Class,
    Interface,
    Module,
    Property,
    Keyword,
    Snippet,
    Constant,
    Enum,
    EnumMember,
    TypeParameter,
    File,
    Folder,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CompletionDto {
    pub label: String,
    pub kind: CompletionKindDto,
    pub detail: Option<String>,
    /// What's inserted, when it isn't the label.
    pub insert: Option<String>,
    pub sort: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CompletionsDto {
    pub items: Vec<CompletionDto>,
    /// More would come with more typed: ask again.
    pub incomplete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HoverDto {
    pub markdown: String,
}

/// Where something is defined.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DefinitionDto {
    /// In the workspace: `path` is workspace-relative, and it can be opened.
    #[serde(rename_all = "camelCase")]
    Workspace { path: String, range: TextRangeDto },
    /// Outside it: `path` is the file's own, for saying where.
    #[serde(rename_all = "camelCase")]
    Outside { path: String, range: TextRangeDto },
}

/// How the open workspace's language server is doing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum LanguageStatusDto {
    Starting,
    Ready,
    #[serde(rename_all = "camelCase")]
    Failed {
        message: String,
    },
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
pub struct WorkspaceArgs {
    pub workspace_id: String,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitDiffArgs {
    pub workspace_id: String,
    pub path: String,
    pub side: DiffSideDto,
}

/// Files to stage, unstage or discard; workspace-relative.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitPathsArgs {
    pub workspace_id: String,
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitCommitArgs {
    pub workspace_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitSwitchArgs {
    pub workspace_id: String,
    pub name: String,
    /// Make the branch from the current commit first.
    pub create: bool,
}

/// A file's text, for the language server.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LspDocumentArgs {
    pub workspace_id: String,
    pub path: String,
    pub text: String,
}

/// A place in a file, for the language server.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LspPositionArgs {
    pub workspace_id: String,
    pub path: String,
    pub line: u32,
    pub character: u32,
}

/// Find in files. A new search cancels the one still running.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchTextArgs {
    pub workspace_id: String,
    pub pattern: String,
    /// A regular expression (Rust syntax) rather than the text as written.
    pub is_regex: bool,
    pub match_case: bool,
    pub whole_word: bool,
    /// Gitignore-style globs; when any are given, only matching files are searched.
    pub include: Vec<String>,
    /// Gitignore-style globs of files to leave out.
    pub exclude: Vec<String>,
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

/// Starts the person's shell in the open workspace's folder.
#[derive(Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalOpenArgs {
    pub workspace_id: String,
    pub cols: u16,
    pub rows: u16,
    /// A Tauri `Channel` for `TerminalEventDto`s.
    #[ts(type = "unknown")]
    pub channel: JavaScriptChannelId,
}

impl std::fmt::Debug for TerminalOpenArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerminalOpenArgs")
            .field("workspace_id", &self.workspace_id)
            .field("cols", &self.cols)
            .field("rows", &self.rows)
            .finish_non_exhaustive()
    }
}

/// Keystrokes or pasted text for a terminal.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalWriteArgs {
    pub id: u32,
    pub data: String,
}

#[derive(Debug, Clone, Copy, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalResizeArgs {
    pub id: u32,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, Copy, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalIdArgs {
    pub id: u32,
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
            ReadOnlyReason::GitInternal => Self::GitInternal,
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

impl From<Span> for SpanDto {
    fn from(span: Span) -> Self {
        Self {
            start: span.start,
            end: span.end,
        }
    }
}

impl From<LineMatch> for LineMatchDto {
    fn from(found: LineMatch) -> Self {
        Self {
            line: found.line,
            column: found.column,
            preview: found.preview,
            spans: found.spans.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<FileMatches> for FileMatchesDto {
    fn from(file: FileMatches) -> Self {
        Self {
            path: file.path.as_str().to_owned(),
            lines: file.lines.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<TextSearchOutcome> for TextSearchDto {
    fn from(outcome: TextSearchOutcome) -> Self {
        Self {
            files: outcome.files.into_iter().map(Into::into).collect(),
            truncated: outcome.truncated,
            cancelled: outcome.cancelled,
        }
    }
}

impl From<FileList> for FileListDto {
    fn from(list: FileList) -> Self {
        Self {
            files: list
                .files
                .iter()
                .map(|path| path.as_str().to_owned())
                .collect(),
            truncated: list.truncated,
        }
    }
}

impl From<GitChange> for GitChangeDto {
    fn from(change: GitChange) -> Self {
        match change {
            GitChange::Modified => Self::Modified,
            GitChange::Added => Self::Added,
            GitChange::Deleted => Self::Deleted,
            GitChange::Renamed => Self::Renamed,
            GitChange::Copied => Self::Copied,
            GitChange::TypeChanged => Self::TypeChanged,
            GitChange::Untracked => Self::Untracked,
            GitChange::Conflicted => Self::Conflicted,
        }
    }
}

impl From<GitFileStatus> for GitFileDto {
    fn from(file: GitFileStatus) -> Self {
        Self {
            path: file.path.as_str().to_owned(),
            from: file.from.map(|from| from.as_str().to_owned()),
            staged: file.staged.map(Into::into),
            unstaged: file.unstaged.map(Into::into),
        }
    }
}

impl From<GitHead> for GitHeadDto {
    fn from(head: GitHead) -> Self {
        Self {
            branch: head.branch,
            commit: head.commit,
            upstream: head.upstream,
            ahead: head.ahead,
            behind: head.behind,
        }
    }
}

impl From<GitState> for GitStateDto {
    fn from(state: GitState) -> Self {
        match state {
            GitState::Unavailable => Self::Unavailable,
            GitState::NotARepository => Self::NotARepository,
            GitState::Repository(GitStatus {
                head,
                files,
                truncated,
            }) => Self::Repository {
                head: head.into(),
                files: files.into_iter().map(Into::into).collect(),
                truncated,
            },
        }
    }
}

impl From<DiffSideDto> for DiffSide {
    fn from(side: DiffSideDto) -> Self {
        match side {
            DiffSideDto::Staged => Self::Staged,
            DiffSideDto::Unstaged => Self::Unstaged,
        }
    }
}

impl From<DiffTexts> for GitDiffDto {
    fn from(texts: DiffTexts) -> Self {
        match texts {
            DiffTexts::Text { original, modified } => Self::Text { original, modified },
            DiffTexts::Binary => Self::Binary,
            DiffTexts::TooLarge { limit } => Self::TooLarge { limit },
        }
    }
}

impl From<GitBranch> for GitBranchDto {
    fn from(branch: GitBranch) -> Self {
        Self {
            name: branch.name,
            current: branch.current,
        }
    }
}

impl From<TerminalEvent> for TerminalEventDto {
    fn from(event: TerminalEvent) -> Self {
        match event {
            TerminalEvent::Output(data) => Self::Output { data },
            TerminalEvent::Exited { code } => Self::Exited { code },
        }
    }
}

impl From<TextPosition> for TextPositionDto {
    fn from(at: TextPosition) -> Self {
        Self {
            line: at.line,
            character: at.character,
        }
    }
}

impl From<TextRange> for TextRangeDto {
    fn from(range: TextRange) -> Self {
        Self {
            start: range.start.into(),
            end: range.end.into(),
        }
    }
}

impl From<Problem> for ProblemDto {
    fn from(problem: Problem) -> Self {
        Self {
            range: problem.range.into(),
            severity: match problem.severity {
                Severity::Error => SeverityDto::Error,
                Severity::Warning => SeverityDto::Warning,
                Severity::Info => SeverityDto::Info,
                Severity::Hint => SeverityDto::Hint,
            },
            message: problem.message,
            source: problem.source,
            code: problem.code,
        }
    }
}

impl From<CompletionKind> for CompletionKindDto {
    fn from(kind: CompletionKind) -> Self {
        match kind {
            CompletionKind::Text => Self::Text,
            CompletionKind::Method => Self::Method,
            CompletionKind::Function => Self::Function,
            CompletionKind::Constructor => Self::Constructor,
            CompletionKind::Field => Self::Field,
            CompletionKind::Variable => Self::Variable,
            CompletionKind::Class => Self::Class,
            CompletionKind::Interface => Self::Interface,
            CompletionKind::Module => Self::Module,
            CompletionKind::Property => Self::Property,
            CompletionKind::Keyword => Self::Keyword,
            CompletionKind::Snippet => Self::Snippet,
            CompletionKind::Constant => Self::Constant,
            CompletionKind::Enum => Self::Enum,
            CompletionKind::EnumMember => Self::EnumMember,
            CompletionKind::TypeParameter => Self::TypeParameter,
            CompletionKind::File => Self::File,
            CompletionKind::Folder => Self::Folder,
            CompletionKind::Other => Self::Other,
        }
    }
}

impl From<Completions> for CompletionsDto {
    fn from(list: Completions) -> Self {
        Self {
            items: list
                .items
                .into_iter()
                .map(|item: Completion| CompletionDto {
                    label: item.label,
                    kind: item.kind.into(),
                    detail: item.detail,
                    insert: item.insert,
                    sort: item.sort,
                })
                .collect(),
            incomplete: list.incomplete,
        }
    }
}

impl From<HoverText> for HoverDto {
    fn from(hover: HoverText) -> Self {
        Self {
            markdown: hover.markdown,
        }
    }
}

impl From<Definition> for DefinitionDto {
    fn from(definition: Definition) -> Self {
        match definition {
            Definition::Workspace { path, range } => Self::Workspace {
                path: path.as_str().to_owned(),
                range: range.into(),
            },
            Definition::Outside { path, range } => Self::Outside {
                path,
                range: range.into(),
            },
        }
    }
}

impl From<LanguageStatus> for LanguageStatusDto {
    fn from(status: LanguageStatus) -> Self {
        match status {
            LanguageStatus::Starting => Self::Starting,
            LanguageStatus::Ready => Self::Ready,
            LanguageStatus::Failed(message) => Self::Failed { message },
        }
    }
}
