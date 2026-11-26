//! What crosses the IPC boundary. Exported to `packages/contracts` with ts-rs,
//! so the TypeScript types are never written by hand.
//!
//! Conventions: camelCase fields, `/`-separated workspace-relative paths,
//! sizes and times as plain numbers (u64 is exported as `number`; every value
//! here is far below 2^53).

use ide_core::agent::{AgentCall, AgentConfig};
use ide_core::assistant::{
    AssistantEvent, AssistantStatus, BedrockAuth, ModelInfo, ProviderKind, ProviderSetup,
    ToolStatus,
};
use ide_core::debug::{Breakpoint, DebugEvent, FrameSource, Scope, StackFrame, Step, Variable};
use ide_core::fs::{
    DirListing, FileNode, FileStamp, LineEnding, NodeKind, ReadOnlyReason, ReadOutcome,
};
use ide_core::git::{
    DiffSide, DiffTexts, GitBranch, GitChange, GitFileStatus, GitHead, GitState, GitStatus,
};
use ide_core::language::{
    CodeAction, Completion, CompletionKind, Completions, Definition, HoverText, LanguageStatus,
    Problem, ResolvedCompletion, Severity, SourceAction, TextEdit, TextPosition, TextRange,
};
use ide_core::workspace::display_path;
use ide_core::{
    EditorSettings, FileList, FileMatches, FsChange, LineHighlight, LineMatch, RecentWorkspace,
    RelPath, Settings, SettingsPatch, Span, TerminalEvent, TextSearchOutcome, Theme, Whitespace,
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
    /// The system it runs on, which places the window's buttons.
    pub os: OsDto,
}

/// The systems the app runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum OsDto {
    Windows,
    Macos,
    Linux,
}

impl OsDto {
    /// The system this build is for.
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else {
            Self::Linux
        }
    }
}

/// Whether `webide` in a terminal runs this app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CommandLineStatusDto {
    /// It's at `path`; `hint` is what's left to do (open a new terminal…).
    Installed {
        path: String,
        hint: Option<String>,
    },
    NotInstalled,
    /// This copy of the app can't put it on the PATH: why.
    Unavailable {
        reason: String,
    },
}

impl From<ide_core::cli::CommandLineStatus> for CommandLineStatusDto {
    fn from(status: ide_core::cli::CommandLineStatus) -> Self {
        use ide_core::cli::CommandLineStatus;
        match status {
            CommandLineStatus::Installed { path, hint } => Self::Installed { path, hint },
            CommandLineStatus::NotInstalled => Self::NotInstalled,
            CommandLineStatus::Unavailable { reason } => Self::Unavailable { reason },
        }
    }
}

/// How the window is shown, which the title bar follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WindowStateDto {
    pub maximized: bool,
    pub fullscreen: bool,
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
    /// The person trusts the authors of its files. Until then it's in
    /// Restricted Mode: nothing runs its own code (git, the debugger, some
    /// language servers).
    pub trusted: bool,
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
    pub editor: EditorSettingsDto,
    /// ESLint checks JavaScript and TypeScript files, in trusted folders.
    pub eslint: bool,
}

/// Where spaces and tabs are drawn as marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum WhitespaceDto {
    None,
    /// In selected text only.
    Selection,
    All,
}

/// How the cursor's line is marked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum LineHighlightDto {
    None,
    Tint,
    Border,
    Both,
}

/// How the editor draws the text's lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditorSettingsDto {
    /// A thin line down each level of indentation.
    pub indent_guides: bool,
    /// A line down the editor at this column (10 to 500); `null` for none.
    pub ruler_column: Option<u16>,
    pub whitespace: WhitespaceDto,
    pub line_highlight: LineHighlightDto,
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
    /// What the debug session of the workspace reports.
    #[serde(rename_all = "camelCase")]
    Debug {
        workspace_id: String,
        event: DebugEventDto,
    },
    /// The workspace's language server started, is ready, or failed.
    #[serde(rename_all = "camelCase")]
    LanguageStatus {
        workspace_id: String,
        /// Which server: "TypeScript", "JSON", "YAML"…
        server: String,
        status: LanguageStatusDto,
    },
    /// An AI agent used a tool of the MCP server.
    AgentCall { call: AgentCallDto },
    /// What a conversation with the IDE's assistant reports.
    Assistant { event: AssistantEventDto },
    /// The window was maximized, restored, or went full screen or back.
    WindowState { state: WindowStateDto },
}

/// The MCP server for AI agents: what the person chose, and whether it runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatusDto {
    pub enabled: bool,
    /// Agents may write files, not only read them.
    pub allow_edits: bool,
    pub port: u16,
    /// Where agents connect, while it runs: `http://127.0.0.1:7341/mcp`.
    pub url: Option<String>,
    /// What agents send as `Authorization: Bearer <token>`.
    pub token: String,
    /// Why it isn't running though enabled (the port is in use…).
    pub error: Option<String>,
}

/// One tool an agent used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentCallDto {
    pub tool: String,
    /// A path, or a search's pattern.
    pub target: Option<String>,
    /// Why it failed, if it did.
    pub error: Option<String>,
}

impl From<AgentCall> for AgentCallDto {
    fn from(call: AgentCall) -> Self {
        Self {
            tool: call.tool,
            target: call.target,
            error: call.error,
        }
    }
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
    /// The folder isn't trusted: git, which can run a repository's own
    /// configured programs, doesn't run in it.
    Restricted,
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

/// A change to a file's text: `range` is replaced by `newText`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TextEditDto {
    pub range: TextRangeDto,
    pub new_text: String,
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
    /// Where what's inserted starts, when not where the word typed does.
    pub replace_from: Option<TextPositionDto>,
    pub sort: Option<String>,
    /// The module it would be imported from, when it isn't in scope yet.
    pub source: Option<String>,
    /// Other changes made with it, such as an import.
    pub edits: Vec<TextEditDto>,
    /// Give it to `lsp_resolve_completion`, when it's picked, for what else
    /// picking it does (the import it adds).
    pub resolve: Option<String>,
}

/// What else picking a completion does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedCompletionDto {
    /// What it does, said: `Add import from "react"`.
    pub detail: Option<String>,
    pub edits: Vec<TextEditDto>,
}

/// A fix the language server offers, with its edits to the file asked about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CodeActionDto {
    pub title: String,
    /// The one the server thinks is meant.
    pub preferred: bool,
    pub edits: Vec<TextEditDto>,
}

/// A change to a whole file the language server makes on request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum SourceActionDto {
    /// Sorts the imports and removes the unused ones.
    OrganizeImports,
    RemoveUnusedImports,
    SortImports,
    /// Every fix the server is sure of.
    FixAll,
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
    /// It runs on Node.js, which isn't installed (or wasn't found).
    NeedsNode,
    #[serde(rename_all = "camelCase")]
    Failed {
        message: String,
    },
    /// It would run the folder's own code, and the folder isn't trusted.
    Restricted,
}

/// What the debugger reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DebugEventDto {
    /// The program wrote this (`stdout`, `stderr`), or the debugger said it (`console`).
    #[serde(rename_all = "camelCase")]
    Output {
        category: String,
        text: String,
    },
    /// Paused: `reason` is `breakpoint`, `step`, `pause`, `exception`, `debugger statement`…
    #[serde(rename_all = "camelCase")]
    Stopped {
        thread_id: i64,
        reason: String,
        description: Option<String>,
    },
    Continued,
    /// The program ended.
    Terminated,
}

/// Where a stack frame is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FrameSourceDto {
    /// In the workspace: `path` is workspace-relative, and it can be opened.
    #[serde(rename_all = "camelCase")]
    Workspace { path: String },
    /// Outside it, or with no file: only named.
    #[serde(rename_all = "camelCase")]
    Outside { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct StackFrameDto {
    pub id: i64,
    pub name: String,
    pub source: Option<FrameSourceDto>,
    /// 1-based.
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScopeDto {
    pub name: String,
    /// For `debug_variables`; 0 when it has none.
    pub reference: i64,
    /// Many variables (the globals): best shown closed.
    pub expensive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct VariableDto {
    pub name: String,
    pub value: String,
    pub kind: Option<String>,
    /// For its children; 0 when it has none.
    pub reference: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BreakpointDto {
    pub line: u32,
    pub verified: bool,
    /// Where it was placed, when that's another line.
    pub actual_line: Option<u32>,
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

/// A completion that was picked, by its `resolve`.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LspResolveArgs {
    pub workspace_id: String,
    pub path: String,
    pub resolve: String,
}

/// Part of a file (a cursor when `start` is `end`), for its fixes.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LspRangeArgs {
    pub workspace_id: String,
    pub path: String,
    pub start: TextPositionDto,
    pub end: TextPositionDto,
}

/// A change to a whole file, asked of the language server.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LspSourceActionArgs {
    pub workspace_id: String,
    pub path: String,
    pub action: SourceActionDto,
}

/// One file's breakpoint lines (1-based).
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileBreakpoints {
    pub path: String,
    pub lines: Vec<u32>,
}

/// Runs a file under the debugger, with breakpoints set before it starts.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DebugStartArgs {
    pub workspace_id: String,
    pub path: String,
    pub breakpoints: Vec<FileBreakpoints>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DebugBreakpointsArgs {
    pub workspace_id: String,
    pub path: String,
    pub lines: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum StepDto {
    Over,
    Into,
    Out,
}

/// Continue, pause, step: on a thread.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DebugThreadArgs {
    pub workspace_id: String,
    pub thread_id: i64,
    #[serde(default)]
    #[ts(optional)]
    pub step: Option<StepDto>,
}

/// A frame's scopes, or a reference's variables.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DebugReferenceArgs {
    pub workspace_id: String,
    pub reference: i64,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DebugEvaluateArgs {
    pub workspace_id: String,
    pub expression: String,
    #[serde(default)]
    #[ts(optional)]
    pub frame_id: Option<i64>,
}

/// The MCP server's settings. It starts, stops or restarts to match.
#[derive(Debug, Clone, Copy, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentConfigArgs {
    pub enabled: bool,
    pub allow_edits: bool,
    pub port: u16,
}

impl From<AgentConfigArgs> for AgentConfig {
    fn from(args: AgentConfigArgs) -> Self {
        Self {
            enabled: args.enabled,
            allow_edits: args.allow_edits,
            port: args.port,
        }
    }
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
    /// All of the editor's settings, or none.
    #[serde(default)]
    #[ts(optional)]
    pub editor: Option<EditorSettingsDto>,
    #[serde(default)]
    #[ts(optional)]
    pub eslint: Option<bool>,
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

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceTrustArgs {
    pub workspace_id: String,
    pub trusted: bool,
    /// Trusts the folder it's in instead, and so every folder there.
    #[serde(default)]
    #[ts(optional)]
    pub parent: Option<bool>,
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
            editor: settings.editor.into(),
            eslint: settings.eslint,
        }
    }
}

impl From<EditorSettings> for EditorSettingsDto {
    fn from(editor: EditorSettings) -> Self {
        Self {
            indent_guides: editor.indent_guides,
            ruler_column: editor.ruler_column,
            whitespace: match editor.whitespace {
                Whitespace::None => WhitespaceDto::None,
                Whitespace::Selection => WhitespaceDto::Selection,
                Whitespace::All => WhitespaceDto::All,
            },
            line_highlight: match editor.line_highlight {
                LineHighlight::None => LineHighlightDto::None,
                LineHighlight::Tint => LineHighlightDto::Tint,
                LineHighlight::Border => LineHighlightDto::Border,
                LineHighlight::Both => LineHighlightDto::Both,
            },
        }
    }
}

impl From<EditorSettingsDto> for EditorSettings {
    fn from(editor: EditorSettingsDto) -> Self {
        Self {
            indent_guides: editor.indent_guides,
            ruler_column: editor.ruler_column,
            whitespace: match editor.whitespace {
                WhitespaceDto::None => Whitespace::None,
                WhitespaceDto::Selection => Whitespace::Selection,
                WhitespaceDto::All => Whitespace::All,
            },
            line_highlight: match editor.line_highlight {
                LineHighlightDto::None => LineHighlight::None,
                LineHighlightDto::Tint => LineHighlight::Tint,
                LineHighlightDto::Border => LineHighlight::Border,
                LineHighlightDto::Both => LineHighlight::Both,
            },
        }
    }
}

impl From<SettingsPatchDto> for SettingsPatch {
    fn from(patch: SettingsPatchDto) -> Self {
        Self {
            theme: patch.theme.map(Into::into),
            read_only_above_bytes: patch.read_only_above_bytes,
            refuse_above_bytes: patch.refuse_above_bytes,
            editor: patch.editor.map(Into::into),
            eslint: patch.eslint,
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

impl From<TextPositionDto> for TextPosition {
    fn from(at: TextPositionDto) -> Self {
        Self {
            line: at.line,
            character: at.character,
        }
    }
}

impl From<TextEdit> for TextEditDto {
    fn from(edit: TextEdit) -> Self {
        Self {
            range: edit.range.into(),
            new_text: edit.new_text,
        }
    }
}

impl From<ResolvedCompletion> for ResolvedCompletionDto {
    fn from(resolved: ResolvedCompletion) -> Self {
        Self {
            detail: resolved.detail,
            edits: resolved.edits.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<CodeAction> for CodeActionDto {
    fn from(action: CodeAction) -> Self {
        Self {
            title: action.title,
            preferred: action.preferred,
            edits: action.edits.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<SourceActionDto> for SourceAction {
    fn from(action: SourceActionDto) -> Self {
        match action {
            SourceActionDto::OrganizeImports => Self::OrganizeImports,
            SourceActionDto::RemoveUnusedImports => Self::RemoveUnusedImports,
            SourceActionDto::SortImports => Self::SortImports,
            SourceActionDto::FixAll => Self::FixAll,
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
                    replace_from: item.replace_from.map(Into::into),
                    sort: item.sort,
                    source: item.source,
                    edits: item.edits.into_iter().map(Into::into).collect(),
                    resolve: item.resolve,
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
            LanguageStatus::NeedsNode => Self::NeedsNode,
            LanguageStatus::Failed(message) => Self::Failed { message },
            LanguageStatus::Restricted => Self::Restricted,
        }
    }
}

impl From<DebugEvent> for DebugEventDto {
    fn from(event: DebugEvent) -> Self {
        match event {
            DebugEvent::Output { category, text } => Self::Output { category, text },
            DebugEvent::Stopped {
                thread_id,
                reason,
                description,
            } => Self::Stopped {
                thread_id,
                reason,
                description,
            },
            DebugEvent::Continued => Self::Continued,
            DebugEvent::Terminated => Self::Terminated,
        }
    }
}

impl From<StackFrame> for StackFrameDto {
    fn from(frame: StackFrame) -> Self {
        Self {
            id: frame.id,
            name: frame.name,
            source: frame.source.map(|source| match source {
                FrameSource::Workspace(path) => FrameSourceDto::Workspace {
                    path: path.as_str().to_owned(),
                },
                FrameSource::Outside(name) => FrameSourceDto::Outside { name },
            }),
            line: frame.line,
            column: frame.column,
        }
    }
}

impl From<Scope> for ScopeDto {
    fn from(scope: Scope) -> Self {
        Self {
            name: scope.name,
            reference: scope.reference,
            expensive: scope.expensive,
        }
    }
}

impl From<Variable> for VariableDto {
    fn from(variable: Variable) -> Self {
        Self {
            name: variable.name,
            value: variable.value,
            kind: variable.kind,
            reference: variable.reference,
        }
    }
}

impl From<Breakpoint> for BreakpointDto {
    fn from(breakpoint: Breakpoint) -> Self {
        Self {
            line: breakpoint.line,
            verified: breakpoint.verified,
            actual_line: breakpoint.actual_line,
        }
    }
}

impl From<StepDto> for Step {
    fn from(step: StepDto) -> Self {
        match step {
            StepDto::Over => Self::Over,
            StepDto::Into => Self::Into,
            StepDto::Out => Self::Out,
        }
    }
}

// ---------- The AI assistant ----------

/// Where the assistant's model is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub enum ProviderKindDto {
    #[serde(rename = "anthropic")]
    Anthropic,
    #[serde(rename = "openai")]
    OpenAi,
    /// Azure AI Foundry (Azure OpenAI).
    #[serde(rename = "azure")]
    AzureFoundry,
    #[serde(rename = "gemini")]
    Gemini,
    #[serde(rename = "ollama")]
    Ollama,
    /// Amazon Bedrock.
    #[serde(rename = "bedrock")]
    Bedrock,
}

/// How Bedrock requests are authorized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum BedrockAuthDto {
    /// The AWS credentials on this machine, from a profile (SSO included).
    Profile,
    /// A Bedrock API key.
    ApiKey,
}

/// The assistant's setup. No secret is in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderSetupDto {
    pub kind: ProviderKindDto,
    /// The model's id, or for Azure the deployment's name. Empty while it's being chosen.
    pub model: String,
    /// Azure: the resource's endpoint. Ollama: the server (`http://127.0.0.1:11434` when null).
    #[serde(default)]
    pub endpoint: Option<String>,
    /// Bedrock: the AWS region (`us-east-1` when null).
    #[serde(default)]
    pub region: Option<String>,
    /// Bedrock with a profile: its name (the default profile when null).
    #[serde(default)]
    pub profile: Option<String>,
    pub bedrock_auth: BedrockAuthDto,
}

/// The saved setup, and whether a key is kept for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AssistantStatusDto {
    /// Null until the assistant is set up.
    pub setup: Option<ProviderSetupDto>,
    /// A key is kept for the setup's provider, in the system's credential store.
    pub has_secret: bool,
}

/// A model to pick.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfoDto {
    pub id: String,
    pub name: Option<String>,
}

/// A setup, with a key typed in the setup (it's kept only by `assistant_save`).
#[derive(Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssistantSetupArgs {
    pub setup: ProviderSetupDto,
    /// Left out: the key already kept, if any.
    #[serde(default)]
    #[ts(optional)]
    pub secret: Option<String>,
}

impl std::fmt::Debug for AssistantSetupArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AssistantSetupArgs")
            .field("setup", &self.setup)
            .field("secret", &self.secret.as_ref().map(|_| "…"))
            .finish()
    }
}

/// The person's message in a conversation; a new id starts one.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssistantSendArgs {
    pub conversation_id: String,
    pub text: String,
    /// The file in front of the person, relative to the workspace.
    #[serde(default)]
    #[ts(optional)]
    pub active_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssistantConversationArgs {
    pub conversation_id: String,
}

/// Accepts or rejects a change the assistant proposed.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssistantDecideArgs {
    pub conversation_id: String,
    pub call_id: String,
    pub accept: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ToolStatusDto {
    Running,
    Done,
    Failed,
}

/// What a conversation reports as it goes. Switch on `kind`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AssistantEventDto {
    /// More of the reply's text (Markdown).
    #[serde(rename_all = "camelCase")]
    Text {
        conversation_id: String,
        delta: String,
    },
    /// A read tool started, finished or failed.
    #[serde(rename_all = "camelCase")]
    Tool {
        conversation_id: String,
        call_id: String,
        name: String,
        /// Its arguments, as JSON.
        input: String,
        status: ToolStatusDto,
        /// The result (shortened), or why it failed.
        output: Option<String>,
    },
    /// A change waiting for the person: `assistant_decide` accepts or rejects it.
    #[serde(rename_all = "camelCase")]
    Proposal {
        conversation_id: String,
        call_id: String,
        path: String,
        /// The file's text now; null for a new file.
        original: Option<String>,
        proposed: String,
        summary: Option<String>,
    },
    /// The person decided; `error` says why an accepted change wasn't written.
    #[serde(rename_all = "camelCase")]
    ProposalDecided {
        conversation_id: String,
        call_id: String,
        accepted: bool,
        error: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Done { conversation_id: String },
    /// The reply stopped: the provider failed, or the person stopped it.
    #[serde(rename_all = "camelCase")]
    Failed {
        conversation_id: String,
        message: String,
    },
}

impl From<ProviderKind> for ProviderKindDto {
    fn from(kind: ProviderKind) -> Self {
        match kind {
            ProviderKind::Anthropic => Self::Anthropic,
            ProviderKind::OpenAi => Self::OpenAi,
            ProviderKind::AzureFoundry => Self::AzureFoundry,
            ProviderKind::Gemini => Self::Gemini,
            ProviderKind::Ollama => Self::Ollama,
            ProviderKind::Bedrock => Self::Bedrock,
        }
    }
}

impl From<ProviderKindDto> for ProviderKind {
    fn from(kind: ProviderKindDto) -> Self {
        match kind {
            ProviderKindDto::Anthropic => Self::Anthropic,
            ProviderKindDto::OpenAi => Self::OpenAi,
            ProviderKindDto::AzureFoundry => Self::AzureFoundry,
            ProviderKindDto::Gemini => Self::Gemini,
            ProviderKindDto::Ollama => Self::Ollama,
            ProviderKindDto::Bedrock => Self::Bedrock,
        }
    }
}

impl From<ProviderSetup> for ProviderSetupDto {
    fn from(setup: ProviderSetup) -> Self {
        Self {
            kind: setup.kind.into(),
            model: setup.model,
            endpoint: setup.endpoint,
            region: setup.region,
            profile: setup.profile,
            bedrock_auth: match setup.bedrock_auth {
                BedrockAuth::Profile => BedrockAuthDto::Profile,
                BedrockAuth::ApiKey => BedrockAuthDto::ApiKey,
            },
        }
    }
}

impl From<ProviderSetupDto> for ProviderSetup {
    fn from(setup: ProviderSetupDto) -> Self {
        Self {
            kind: setup.kind.into(),
            model: setup.model,
            endpoint: setup.endpoint,
            region: setup.region,
            profile: setup.profile,
            bedrock_auth: match setup.bedrock_auth {
                BedrockAuthDto::Profile => BedrockAuth::Profile,
                BedrockAuthDto::ApiKey => BedrockAuth::ApiKey,
            },
        }
    }
}

impl From<AssistantStatus> for AssistantStatusDto {
    fn from(status: AssistantStatus) -> Self {
        Self {
            setup: status.setup.map(Into::into),
            has_secret: status.has_secret,
        }
    }
}

impl From<ModelInfo> for ModelInfoDto {
    fn from(model: ModelInfo) -> Self {
        Self {
            id: model.id,
            name: model.name,
        }
    }
}

impl From<AssistantEvent> for AssistantEventDto {
    fn from(event: AssistantEvent) -> Self {
        match event {
            AssistantEvent::Text {
                conversation,
                delta,
            } => Self::Text {
                conversation_id: conversation,
                delta,
            },
            AssistantEvent::Tool {
                conversation,
                call_id,
                name,
                input,
                status,
                output,
            } => Self::Tool {
                conversation_id: conversation,
                call_id,
                name,
                input,
                status: match status {
                    ToolStatus::Running => ToolStatusDto::Running,
                    ToolStatus::Done => ToolStatusDto::Done,
                    ToolStatus::Failed => ToolStatusDto::Failed,
                },
                output,
            },
            AssistantEvent::Proposal {
                conversation,
                call_id,
                path,
                original,
                proposed,
                summary,
            } => Self::Proposal {
                conversation_id: conversation,
                call_id,
                path,
                original,
                proposed,
                summary,
            },
            AssistantEvent::ProposalDecided {
                conversation,
                call_id,
                accepted,
                error,
            } => Self::ProposalDecided {
                conversation_id: conversation,
                call_id,
                accepted,
                error,
            },
            AssistantEvent::Done { conversation } => Self::Done {
                conversation_id: conversation,
            },
            AssistantEvent::Failed {
                conversation,
                message,
            } => Self::Failed {
                conversation_id: conversation,
                message,
            },
        }
    }
}
