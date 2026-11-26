//! What each command does, as plain functions over [`IpcState`].
//! No Tauri here, so they're tested with fake ports.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use ide_core::agent::{AgentConfig, AgentHost, AgentServer, AgentSink, AgentWorkbench};
use ide_core::assistant::{AssistantHost, AssistantSink, ChatContext};
use ide_core::cli::{CommandLine, Launch};
use ide_core::debug::{DebugAdapters, DebugEvent, DebugSession};
use ide_core::fs::ReadLimits;
use ide_core::language::{
    LanguageServer, LanguageServers, LanguageStatus, StartOptions, TextPosition, TextRange,
};
use ide_core::terminal::{MAX_TERMINALS, MAX_WRITE_BYTES};
use ide_core::workspace::display_path;
use ide_core::{
    CancelFlag, FsWatcher, GitClient, IdeError, RelPath, TerminalHost, TerminalSession,
    TerminalSink, TerminalSize, TextQuery, WatchEvent, WatchGuard, WatchSink, WorkspaceFs,
    WorkspaceRoot, WorkspaceSearch, WorkspaceStore,
};

use crate::dto::{
    AgentConfigArgs, AgentStatusDto, AppInfoDto, AssistantConversationArgs, AssistantDecideArgs,
    AssistantSendArgs, AssistantSetupArgs, AssistantStatusDto, BackendEventDto, BreakpointDto,
    CodeActionDto, CommandLineStatusDto, CompletionsDto, DebugBreakpointsArgs, DebugEvaluateArgs,
    DebugEventDto, DebugReferenceArgs, DebugStartArgs, DebugThreadArgs, DefinitionDto,
    DirListingDto, FileListDto, FileStampDto, GitBranchDto, GitCommitArgs, GitDiffArgs, GitDiffDto,
    GitPathsArgs, GitStateDto, GitSwitchArgs, HoverDto, LanguageStatusDto, LspDocumentArgs,
    LspPositionArgs, LspRangeArgs, LspResolveArgs, LspSourceActionArgs, ModelInfoDto,
    OpenRecentArgs, ProblemDto, ReadFileDto, RecentWorkspaceDto, ResolvedCompletionDto, ScopeDto,
    SearchTextArgs, SettingsDto, SettingsPatchDto, StackFrameDto, TerminalDto, TerminalIdArgs,
    TerminalResizeArgs, TerminalWriteArgs, TextEditDto, TextSearchDto, VariableDto, WorkspaceArgs,
    WorkspaceDto, WorkspacePathArgs, WorkspaceTrustArgs, WriteFileArgs,
};
use crate::error::IpcError;

/// The implementations `ide-ipc` runs on, built by the composition root.
#[derive(Clone)]
pub struct Services {
    pub fs: Arc<dyn WorkspaceFs>,
    pub workspace: Arc<dyn WorkspaceStore>,
    pub watcher: Arc<dyn FsWatcher>,
    pub search: Arc<dyn WorkspaceSearch>,
    pub git: Arc<dyn GitClient>,
    pub terminals: Arc<dyn TerminalHost>,
    pub languages: Arc<dyn LanguageServers>,
    pub debuggers: Arc<dyn DebugAdapters>,
    pub agents: Arc<dyn AgentHost>,
    pub assistant: Arc<dyn AssistantHost>,
    /// The `webide` command, on the PATH or not.
    pub command_line: Arc<dyn CommandLine>,
    /// What the terminal asked to open, when the app was started as `webide <path>`.
    pub launch: Option<Launch>,
}

impl std::fmt::Debug for Services {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Services").finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
struct OpenWorkspace {
    id: String,
    root: WorkspaceRoot,
    /// Its files' authors are trusted (`WorkspaceStore::is_trusted`).
    trusted: bool,
}

impl OpenWorkspace {
    fn dto(&self) -> WorkspaceDto {
        WorkspaceDto {
            id: self.id.clone(),
            name: self.root.name().to_owned(),
            path: self.root.display_path(),
            trusted: self.trusted,
        }
    }
}

/// A running terminal, and the workspace it belongs to.
struct StoredTerminal {
    workspace_id: String,
    session: Box<dyn TerminalSession>,
}

/// Delivers an event to the webview, if one is listening.
pub type Emit = Arc<dyn Fn(BackendEventDto) + Send + Sync>;

/// Shared by every command.
pub struct IpcState {
    services: Services,
    app_info: AppInfoDto,
    /// Shared with the MCP server, which answers about the open workspace.
    open: Arc<RwLock<Option<OpenWorkspace>>>,
    next_id: AtomicU64,
    emit: Emit,
    /// The watch on the open workspace; replacing it stops the old one.
    watch: Mutex<Option<WatchGuard>>,
    /// The text search running now, if any. A new one, or a new workspace, cancels it.
    search: Mutex<Option<CancelFlag>>,
    /// Open terminals by id, with the workspace each was opened in. Dropping
    /// one ends its shell.
    terminals: Mutex<HashMap<u32, StoredTerminal>>,
    next_terminal: AtomicU64,
    /// The open workspace's language server, started when a file it handles
    /// is first opened. Held while it starts, so it starts once.
    language: Mutex<Language>,
    /// The program running under the debugger, if any: one at a time.
    debug: Mutex<Option<DebugRun>>,
    /// The MCP server for AI agents, while it runs, or why it couldn't start.
    agent: Mutex<AgentRun>,
    /// The file `webide <file>` named, until the page takes it.
    launch_file: Mutex<Option<String>>,
}

#[derive(Default)]
struct AgentRun {
    server: Option<Box<dyn AgentServer>>,
    error: Option<String>,
}

/// What the MCP server reads: the workspace open now, and the read limits.
struct Workbench {
    open: Arc<RwLock<Option<OpenWorkspace>>>,
    store: Arc<dyn WorkspaceStore>,
}

impl AgentWorkbench for Workbench {
    fn root(&self) -> Option<WorkspaceRoot> {
        self.open
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map(|open| open.root.clone())
    }

    fn read_limits(&self) -> ReadLimits {
        self.store.settings().unwrap_or_default().read_limits()
    }
}

struct DebugRun {
    workspace_id: String,
    session: Arc<dyn DebugSession>,
}

/// The language server of one workspace.
#[derive(Default)]
enum Language {
    #[default]
    NotStarted,
    Running {
        workspace_id: String,
        server: Arc<dyn LanguageServer>,
    },
    /// It couldn't start; not tried again until the workspace opens again (or
    /// `lsp_restart`).
    Failed { workspace_id: String },
}

impl std::fmt::Debug for IpcState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IpcState")
            .field("open", &self.open)
            .finish_non_exhaustive()
    }
}

impl IpcState {
    pub fn new(services: Services, app_info: AppInfoDto, emit: Emit) -> Self {
        Self {
            services,
            app_info,
            open: Arc::new(RwLock::new(None)),
            next_id: AtomicU64::new(1),
            emit,
            watch: Mutex::new(None),
            search: Mutex::new(None),
            terminals: Mutex::new(HashMap::new()),
            next_terminal: AtomicU64::new(1),
            language: Mutex::new(Language::NotStarted),
            debug: Mutex::new(None),
            agent: Mutex::new(AgentRun::default()),
            launch_file: Mutex::new(None),
        }
    }

    /// Opens what `webide <path>` named, as the app starts: the folder, and
    /// the file for the page to open. A folder that can't be opened leaves
    /// the app as it would start without one.
    pub fn open_launch(&self) {
        let Some(launch) = self.services.launch.clone() else {
            return;
        };
        let Ok(root) = self.services.workspace.open(&launch.folder) else {
            return;
        };
        self.set_open(root);
        // The file is in the folder itself: its name is its path there.
        let name = launch
            .file
            .as_deref()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .and_then(|name| RelPath::parse(name).ok());
        *self
            .launch_file
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = name.map(|name| name.as_str().to_owned());
    }

    /// Starts the MCP server, if the person turned it on. At startup.
    pub fn start_agents(&self) {
        if let Ok(config) = self.services.agents.config() {
            self.apply_agents(&config);
        }
    }

    /// Stops the MCP server, then starts it again as `config` says. Why it
    /// couldn't start is kept for `agent_status`.
    fn apply_agents(&self, config: &AgentConfig) {
        let mut run = self.agent.lock().unwrap_or_else(PoisonError::into_inner);
        // Stopped first: the new one may want the same port.
        run.server = None;
        run.error = None;
        if !config.enabled {
            return;
        }
        let emit = Arc::clone(&self.emit);
        let sink: AgentSink = Arc::new(move |call| {
            emit(BackendEventDto::AgentCall { call: call.into() });
        });
        let workbench = self.workbench();
        let started = self
            .services
            .agents
            .token()
            .and_then(|token| self.services.agents.serve(config, &token, workbench, sink));
        match started {
            Ok(server) => run.server = Some(server),
            Err(error) => run.error = Some(error.to_string()),
        }
    }

    /// What the MCP server and the assistant read: the workspace open now.
    fn workbench(&self) -> Arc<Workbench> {
        Arc::new(Workbench {
            open: Arc::clone(&self.open),
            store: Arc::clone(&self.services.workspace),
        })
    }

    fn agent_status(&self) -> Result<AgentStatusDto, IdeError> {
        let config = self.services.agents.config()?;
        let token = self.services.agents.token()?;
        let run = self.agent.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(AgentStatusDto {
            enabled: config.enabled,
            allow_edits: config.allow_edits,
            port: config.port,
            url: run.server.as_ref().map(|server| server.url()),
            token,
            error: run.error.clone(),
        })
    }

    /// Ends the program running under the debugger, if any.
    pub fn stop_debugging(&self) {
        let old = self
            .debug
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        // Ended outside the lock: it waits for the program and the debugger to go.
        drop(old);
    }

    fn debugging(&self, workspace_id: &str) -> Result<Arc<dyn DebugSession>, IdeError> {
        self.root_for(workspace_id)?;
        match &*self.debug.lock().unwrap_or_else(PoisonError::into_inner) {
            Some(run) if run.workspace_id == workspace_id => Ok(Arc::clone(&run.session)),
            _ => Err(IdeError::InvalidArgument(
                "nothing is being debugged".to_owned(),
            )),
        }
    }

    /// Stops the language server: another workspace opened, or the page started again.
    pub fn stop_language(&self) {
        let old =
            std::mem::take(&mut *self.language.lock().unwrap_or_else(PoisonError::into_inner));
        // Stopped outside the lock: it asks the server to leave, which takes a moment.
        drop(old);
    }

    /// The workspace's language server, started if it isn't yet. `None`
    /// when it couldn't start (its status said why).
    fn language_for(
        &self,
        workspace_id: &str,
    ) -> Result<Option<Arc<dyn LanguageServer>>, IdeError> {
        let root = self.root_for(workspace_id)?;
        let mut language = self.language.lock().unwrap_or_else(PoisonError::into_inner);
        match &*language {
            Language::Running {
                workspace_id: id,
                server,
            } if id == workspace_id => {
                return Ok(Some(Arc::clone(server)));
            }
            Language::Failed { workspace_id: id } if id == workspace_id => return Ok(None),
            _ => {}
        }
        let emit = Arc::clone(&self.emit);
        let id = workspace_id.to_owned();
        let status: ide_core::language::LanguageSink =
            Arc::new(move |server: &str, status: LanguageStatus| {
                emit(match LanguageStatusDto::from_status(status) {
                    Some(status) => BackendEventDto::LanguageStatus {
                        workspace_id: id.clone(),
                        server: server.to_owned(),
                        status,
                    },
                    None => BackendEventDto::LanguageProblemsChanged {
                        workspace_id: id.clone(),
                    },
                });
            });
        let options = StartOptions {
            trusted: self.is_trusted(),
            eslint: self
                .services
                .workspace
                .settings()
                .is_ok_and(|settings| settings.eslint),
        };
        match self.services.languages.start(&root, options, status) {
            Ok(server) => {
                let server: Arc<dyn LanguageServer> = Arc::from(server);
                *language = Language::Running {
                    workspace_id: workspace_id.to_owned(),
                    server: Arc::clone(&server),
                };
                Ok(Some(server))
            }
            Err(_) => {
                *language = Language::Failed {
                    workspace_id: workspace_id.to_owned(),
                };
                Ok(None)
            }
        }
    }

    /// Ends every terminal: another workspace opened, or the page started again.
    pub fn close_terminals(&self) {
        let closing: Vec<_> = self
            .terminals
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain()
            .collect();
        // Dropped outside the lock: ending a shell can take a moment.
        drop(closing);
    }

    /// Cancels the running search, if any, and returns the flag for a new one.
    fn replace_search(&self) -> CancelFlag {
        let flag = CancelFlag::new();
        let mut slot = self.search.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(previous) = slot.replace(flag.clone()) {
            previous.cancel();
        }
        flag
    }

    /// Makes `root` the open workspace, under a new id, and watches it instead
    /// of the previous one.
    fn set_open(&self, root: WorkspaceRoot) -> WorkspaceDto {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed).to_string();
        // Not trusted when that can't be read: the safe way to fail.
        let trusted = self.services.workspace.is_trusted(&root).unwrap_or(false);
        let open = OpenWorkspace {
            id: id.clone(),
            root: root.clone(),
            trusted,
        };
        let dto = open.dto();
        let mut watch = self.watch.lock().unwrap_or_else(PoisonError::into_inner);
        // Stop the old watch first, so its last events can't follow the new id.
        watch.take();
        if let Some(search) = self
            .search
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            search.cancel();
        }
        *watch = Some(self.services.watcher.watch(&root, self.sink(id.clone())));
        *self.open.write().unwrap_or_else(PoisonError::into_inner) = Some(open);
        // The old workspace's terminals were in its folder. Closed after the
        // new one is open, so a terminal still starting for the old one is
        // either closed here or refused when it's stored (`terminal_open`).
        self.close_terminals();
        self.stop_language();
        self.stop_debugging();
        dto
    }

    /// Where the watch of workspace `id` reports: to the webview, tagged with `id`.
    fn sink(&self, id: String) -> WatchSink {
        let emit = Arc::clone(&self.emit);
        Arc::new(move |event| {
            emit(match event {
                WatchEvent::Changed { changes, rescan } => BackendEventDto::FilesChanged {
                    workspace_id: id.clone(),
                    changes: changes.into_iter().map(Into::into).collect(),
                    rescan,
                },
                WatchEvent::Failed { message } => BackendEventDto::WatchFailed {
                    workspace_id: id.clone(),
                    message,
                },
            });
        })
    }

    fn current(&self) -> Option<OpenWorkspace> {
        self.open
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The root of the open workspace, for what runs its own code: only
    /// once the person trusts it.
    fn trusted_root(&self, workspace_id: &str) -> Result<WorkspaceRoot, IdeError> {
        let root = self.root_for(workspace_id)?;
        if self.is_trusted() {
            Ok(root)
        } else {
            Err(IdeError::Restricted)
        }
    }

    /// Whether the open workspace is trusted.
    fn is_trusted(&self) -> bool {
        self.current().is_some_and(|open| open.trusted)
    }

    /// The root of the open workspace, if `workspace_id` is still the open one.
    fn root_for(&self, workspace_id: &str) -> Result<WorkspaceRoot, IdeError> {
        match self.current() {
            None => Err(IdeError::NoWorkspace),
            Some(open) if open.id == workspace_id => Ok(open.root),
            Some(_) => Err(IdeError::WorkspaceChanged),
        }
    }
}

// Every handler is `fn(&IpcState[, Args]) -> Result<Output, IpcError>`; the
// command list in `commands.rs` checks each against its declared types.

pub fn app_info(state: &IpcState) -> Result<AppInfoDto, IpcError> {
    Ok(state.app_info.clone())
}

/// Opens a folder the person chose in the native folder picker.
pub fn open_picked(state: &IpcState, path: &Path) -> Result<WorkspaceDto, IpcError> {
    let root = state.services.workspace.open(path)?;
    Ok(state.set_open(root))
}

/// Opens a folder from the recent list. The webview can't name any other
/// folder: a root is only ever chosen by the person, in the native picker.
pub fn open_recent_workspace(
    state: &IpcState,
    args: OpenRecentArgs,
) -> Result<WorkspaceDto, IpcError> {
    let recent = state.services.workspace.recent()?;
    let entry = recent
        .into_iter()
        .find(|entry| display_path(&entry.path) == args.path)
        .ok_or(IdeError::NotARecentWorkspace)?;
    let root = state.services.workspace.open(&entry.path)?;
    Ok(state.set_open(root))
}

pub fn recent_workspaces(state: &IpcState) -> Result<Vec<RecentWorkspaceDto>, IpcError> {
    Ok(state
        .services
        .workspace
        .recent()?
        .iter()
        .map(Into::into)
        .collect())
}

/// Trusts the open workspace's folder (or, with `parent`, the folder it's
/// in), or stops trusting it. What runs its own code starts or stops with
/// it: the language servers start again, and a debugged program ends.
pub fn workspace_trust(
    state: &IpcState,
    args: WorkspaceTrustArgs,
) -> Result<WorkspaceDto, IpcError> {
    let root = state.root_for(&args.workspace_id)?;
    let folder = match args.parent {
        Some(true) => root.path().parent().unwrap_or(root.path()),
        _ => root.path(),
    };
    state.services.workspace.set_trusted(folder, args.trusted)?;
    let trusted = state.services.workspace.is_trusted(&root)?;
    let dto = {
        let mut open = state.open.write().unwrap_or_else(PoisonError::into_inner);
        match open.as_mut() {
            Some(open) if open.id == args.workspace_id => {
                open.trusted = trusted;
                open.dto()
            }
            _ => return Err(IdeError::WorkspaceChanged.into()),
        }
    };
    state.stop_language();
    if !trusted {
        state.stop_debugging();
    }
    Ok(dto)
}

/// The file `webide <file>` named, once: later calls (a reloaded page) get none.
pub fn take_launch_file(state: &IpcState) -> Result<Option<String>, IpcError> {
    Ok(state
        .launch_file
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take())
}

/// Whether `webide` in a terminal runs this app.
pub fn command_line_status(state: &IpcState) -> Result<CommandLineStatusDto, IpcError> {
    Ok(state.services.command_line.status().into())
}

/// Puts `webide` on the PATH (the system may ask for permission).
pub fn command_line_install(state: &IpcState) -> Result<CommandLineStatusDto, IpcError> {
    Ok(state.services.command_line.install()?.into())
}

/// Takes `webide` off the PATH, if this app put it there.
pub fn command_line_uninstall(state: &IpcState) -> Result<CommandLineStatusDto, IpcError> {
    Ok(state.services.command_line.uninstall()?.into())
}

pub fn current_workspace(state: &IpcState) -> Result<Option<WorkspaceDto>, IpcError> {
    Ok(state.current().map(|open| open.dto()))
}

pub fn list_directory(
    state: &IpcState,
    args: WorkspacePathArgs,
) -> Result<DirListingDto, IpcError> {
    let path = RelPath::parse(&args.path)?;
    let root = state.root_for(&args.workspace_id)?;
    Ok(state.services.fs.list_dir(&root, &path)?.into())
}

pub fn read_file(state: &IpcState, args: WorkspacePathArgs) -> Result<ReadFileDto, IpcError> {
    let path = RelPath::parse(&args.path)?;
    let root = state.root_for(&args.workspace_id)?;
    let limits = state.services.workspace.settings()?.read_limits();
    let outcome = state.services.fs.read_file(&root, &path, limits)?;
    Ok(ReadFileDto::from_outcome(&path, outcome))
}

pub fn write_file(state: &IpcState, args: WriteFileArgs) -> Result<FileStampDto, IpcError> {
    let path = RelPath::parse(&args.path)?;
    let root = state.root_for(&args.workspace_id)?;
    Ok(state
        .services
        .fs
        .write_file(&root, &path, &args.text)?
        .into())
}

pub fn get_settings(state: &IpcState) -> Result<SettingsDto, IpcError> {
    Ok(state.services.workspace.settings()?.into())
}

/// Saves a change to the settings. Turning ESLint on or off starts the
/// language servers again, with it or without it, at the next file.
pub fn update_settings(state: &IpcState, patch: SettingsPatchDto) -> Result<SettingsDto, IpcError> {
    let before = state.services.workspace.settings()?.eslint;
    let settings = state.services.workspace.update_settings(&patch.into())?;
    if settings.eslint != before {
        state.stop_language();
    }
    Ok(settings.into())
}

/// Find in files. Files too large to open (over the don't-open size) aren't
/// searched either.
pub fn search_text(state: &IpcState, args: SearchTextArgs) -> Result<TextSearchDto, IpcError> {
    let root = state.root_for(&args.workspace_id)?;
    let max_file_bytes = state
        .services
        .workspace
        .settings()?
        .read_limits()
        .refuse_above;
    let query = TextQuery {
        pattern: args.pattern,
        is_regex: args.is_regex,
        match_case: args.match_case,
        whole_word: args.whole_word,
        include: args.include,
        exclude: args.exclude,
        max_file_bytes,
    };
    let cancel = state.replace_search();
    Ok(state
        .services
        .search
        .search_text(&root, &query, &cancel)?
        .into())
}

/// The workspace's files, for quick open.
pub fn list_files(state: &IpcState, args: WorkspaceArgs) -> Result<FileListDto, IpcError> {
    let root = state.root_for(&args.workspace_id)?;
    Ok(state
        .services
        .search
        .list_files(&root, &CancelFlag::new())?
        .into())
}

fn paths(list: &[String]) -> Result<Vec<RelPath>, IpcError> {
    list.iter()
        .map(|path| RelPath::parse(path).map_err(IpcError::from))
        .collect()
}

/// Whether git can be used in the workspace, and its branch and changes.
pub fn git_state(state: &IpcState, args: WorkspaceArgs) -> Result<GitStateDto, IpcError> {
    let root = state.root_for(&args.workspace_id)?;
    if !state.is_trusted() {
        return Ok(GitStateDto::Restricted);
    }
    Ok(state.services.git.state(&root)?.into())
}

/// The two versions of a file, each no larger than the "don't open above" size.
pub fn git_diff(state: &IpcState, args: GitDiffArgs) -> Result<GitDiffDto, IpcError> {
    let path = RelPath::parse(&args.path)?;
    let root = state.trusted_root(&args.workspace_id)?;
    let limit = state
        .services
        .workspace
        .settings()?
        .read_limits()
        .refuse_above;
    Ok(state
        .services
        .git
        .diff_texts(&root, &path, args.side.into(), limit)?
        .into())
}

pub fn git_stage(state: &IpcState, args: GitPathsArgs) -> Result<(), IpcError> {
    let paths = paths(&args.paths)?;
    let root = state.trusted_root(&args.workspace_id)?;
    Ok(state.services.git.stage(&root, &paths)?)
}

pub fn git_unstage(state: &IpcState, args: GitPathsArgs) -> Result<(), IpcError> {
    let paths = paths(&args.paths)?;
    let root = state.trusted_root(&args.workspace_id)?;
    Ok(state.services.git.unstage(&root, &paths)?)
}

pub fn git_discard(state: &IpcState, args: GitPathsArgs) -> Result<(), IpcError> {
    let paths = paths(&args.paths)?;
    let root = state.trusted_root(&args.workspace_id)?;
    Ok(state.services.git.discard(&root, &paths)?)
}

pub fn git_commit(state: &IpcState, args: GitCommitArgs) -> Result<(), IpcError> {
    let root = state.trusted_root(&args.workspace_id)?;
    Ok(state.services.git.commit(&root, &args.message)?)
}

pub fn git_branches(state: &IpcState, args: WorkspaceArgs) -> Result<Vec<GitBranchDto>, IpcError> {
    let root = state.trusted_root(&args.workspace_id)?;
    Ok(state
        .services
        .git
        .branches(&root)?
        .into_iter()
        .map(Into::into)
        .collect())
}

pub fn git_switch_branch(state: &IpcState, args: GitSwitchArgs) -> Result<(), IpcError> {
    let root = state.trusted_root(&args.workspace_id)?;
    Ok(state
        .services
        .git
        .switch_branch(&root, &args.name, args.create)?)
}

pub fn git_init(state: &IpcState, args: WorkspaceArgs) -> Result<(), IpcError> {
    let root = state.trusted_root(&args.workspace_id)?;
    Ok(state.services.git.init(&root)?)
}

/// Starts the person's shell in the open workspace's folder; its output and
/// its exit go to `sink`.
pub fn terminal_open(
    state: &IpcState,
    workspace_id: &str,
    cols: u16,
    rows: u16,
    sink: TerminalSink,
) -> Result<TerminalDto, IpcError> {
    let root = state.root_for(workspace_id)?;
    let size = TerminalSize::new(cols, rows)?;
    if state
        .terminals
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .len()
        >= MAX_TERMINALS
    {
        return Err(IdeError::InvalidArgument(format!(
            "at most {MAX_TERMINALS} terminals can be open at once"
        ))
        .into());
    }
    // The folder as people write it: shells don't take Windows' `\\?\` paths.
    let cwd = std::path::PathBuf::from(root.display_path());
    let session = state.services.terminals.spawn(&cwd, size, sink)?;
    let id = u32::try_from(state.next_terminal.fetch_add(1, Ordering::Relaxed)).unwrap_or(u32::MAX);
    let shell = session.shell().to_owned();
    let mut terminals = state
        .terminals
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    // Starting the shell took a moment: if another workspace opened meanwhile,
    // this one's terminal mustn't outlive it.
    if state.current().is_none_or(|open| open.id != workspace_id) {
        drop(terminals);
        drop(session);
        return Err(IdeError::WorkspaceChanged.into());
    }
    terminals.insert(
        id,
        StoredTerminal {
            workspace_id: workspace_id.to_owned(),
            session,
        },
    );
    Ok(TerminalDto { id, shell })
}

fn with_terminal<T>(
    state: &IpcState,
    id: u32,
    work: impl FnOnce(&dyn TerminalSession) -> Result<T, IdeError>,
) -> Result<T, IpcError> {
    let terminals = state
        .terminals
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let stored = terminals.get(&id).ok_or(IdeError::NotFound)?;
    // Only the open workspace's terminals take input.
    if state
        .current()
        .is_none_or(|open| open.id != stored.workspace_id)
    {
        return Err(IdeError::WorkspaceChanged.into());
    }
    Ok(work(stored.session.as_ref())?)
}

pub fn terminal_write(state: &IpcState, args: TerminalWriteArgs) -> Result<(), IpcError> {
    if args.data.len() > MAX_WRITE_BYTES {
        return Err(IdeError::InvalidArgument(format!(
            "at most {MAX_WRITE_BYTES} bytes can be sent to a terminal at once"
        ))
        .into());
    }
    with_terminal(state, args.id, |session| {
        session.write(args.data.as_bytes())
    })
}

pub fn terminal_resize(state: &IpcState, args: TerminalResizeArgs) -> Result<(), IpcError> {
    let size = TerminalSize::new(args.cols, args.rows)?;
    with_terminal(state, args.id, |session| session.resize(size))
}

/// Ends a terminal's shell, and forgets it.
pub fn terminal_close(state: &IpcState, args: TerminalIdArgs) -> Result<(), IpcError> {
    let session = state
        .terminals
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&args.id)
        .ok_or(IdeError::NotFound)?;
    drop(session);
    Ok(())
}

/// The server for a file, if one handles it and it's running.
fn language_server(
    state: &IpcState,
    workspace_id: &str,
    path: &RelPath,
) -> Result<Option<Arc<dyn LanguageServer>>, IpcError> {
    if !state.services.languages.handles(path) {
        // Still checks the workspace, as every command does.
        state.root_for(workspace_id)?;
        return Ok(None);
    }
    Ok(state.language_for(workspace_id)?)
}

/// The file is open in the editor: the language server (started now if need
/// be) reads it from here on.
pub fn lsp_open(state: &IpcState, args: LspDocumentArgs) -> Result<(), IpcError> {
    let path = RelPath::parse(&args.path)?;
    match language_server(state, &args.workspace_id, &path)? {
        Some(server) => Ok(server.open(&path, &args.text)?),
        None => Ok(()),
    }
}

pub fn lsp_change(state: &IpcState, args: LspDocumentArgs) -> Result<(), IpcError> {
    let path = RelPath::parse(&args.path)?;
    match language_server(state, &args.workspace_id, &path)? {
        Some(server) => Ok(server.change(&path, &args.text)?),
        None => Ok(()),
    }
}

pub fn lsp_close(state: &IpcState, args: WorkspacePathArgs) -> Result<(), IpcError> {
    let path = RelPath::parse(&args.path)?;
    match language_server(state, &args.workspace_id, &path)? {
        Some(server) => Ok(server.close(&path)?),
        None => Ok(()),
    }
}

pub fn lsp_problems(
    state: &IpcState,
    args: WorkspacePathArgs,
) -> Result<Vec<ProblemDto>, IpcError> {
    let path = RelPath::parse(&args.path)?;
    Ok(match language_server(state, &args.workspace_id, &path)? {
        Some(server) => server
            .problems(&path)?
            .into_iter()
            .map(Into::into)
            .collect(),
        None => Vec::new(),
    })
}

fn at(args: &LspPositionArgs) -> TextPosition {
    TextPosition {
        line: args.line,
        character: args.character,
    }
}

pub fn lsp_completions(
    state: &IpcState,
    args: LspPositionArgs,
) -> Result<CompletionsDto, IpcError> {
    let path = RelPath::parse(&args.path)?;
    Ok(match language_server(state, &args.workspace_id, &path)? {
        Some(server) => server.completions(&path, at(&args))?.into(),
        None => CompletionsDto {
            items: Vec::new(),
            incomplete: false,
        },
    })
}

pub fn lsp_hover(state: &IpcState, args: LspPositionArgs) -> Result<Option<HoverDto>, IpcError> {
    let path = RelPath::parse(&args.path)?;
    Ok(match language_server(state, &args.workspace_id, &path)? {
        Some(server) => server.hover(&path, at(&args))?.map(Into::into),
        None => None,
    })
}

pub fn lsp_definition(
    state: &IpcState,
    args: LspPositionArgs,
) -> Result<Vec<DefinitionDto>, IpcError> {
    let path = RelPath::parse(&args.path)?;
    Ok(match language_server(state, &args.workspace_id, &path)? {
        Some(server) => server
            .definition(&path, at(&args))?
            .into_iter()
            .map(Into::into)
            .collect(),
        None => Vec::new(),
    })
}

/// The longest `resolve` a completion has: the server's tokens are short.
const MAX_RESOLVE_LEN: usize = 64;

/// What else picking a completion does: the import it adds.
pub fn lsp_resolve_completion(
    state: &IpcState,
    args: LspResolveArgs,
) -> Result<ResolvedCompletionDto, IpcError> {
    let path = RelPath::parse(&args.path)?;
    if args.resolve.len() > MAX_RESOLVE_LEN {
        return Err(IdeError::InvalidArgument("that completion isn't known".to_owned()).into());
    }
    Ok(match language_server(state, &args.workspace_id, &path)? {
        Some(server) => server.resolve_completion(&path, &args.resolve)?.into(),
        None => ResolvedCompletionDto {
            detail: None,
            edits: Vec::new(),
        },
    })
}

/// The fixes for the problems in part of a file.
pub fn lsp_fixes(state: &IpcState, args: LspRangeArgs) -> Result<Vec<CodeActionDto>, IpcError> {
    let path = RelPath::parse(&args.path)?;
    let (start, end): (TextPosition, TextPosition) = (args.start.into(), args.end.into());
    if end < start {
        return Err(IdeError::InvalidArgument("the range ends before it starts".to_owned()).into());
    }
    Ok(match language_server(state, &args.workspace_id, &path)? {
        Some(server) => server
            .fixes(&path, TextRange { start, end })?
            .into_iter()
            .map(Into::into)
            .collect(),
        None => Vec::new(),
    })
}

/// The edits that organize a file's imports (or make another source action);
/// `None` when the server doesn't offer it there.
pub fn lsp_source_action(
    state: &IpcState,
    args: LspSourceActionArgs,
) -> Result<Option<Vec<TextEditDto>>, IpcError> {
    let path = RelPath::parse(&args.path)?;
    Ok(match language_server(state, &args.workspace_id, &path)? {
        Some(server) => server
            .source_action(&path, args.action.into())?
            .map(|edits| edits.into_iter().map(Into::into).collect()),
        None => None,
    })
}

/// Stops the workspace's language server; it starts again when it's next needed.
pub fn lsp_restart(state: &IpcState, args: WorkspaceArgs) -> Result<(), IpcError> {
    state.root_for(&args.workspace_id)?;
    state.stop_language();
    Ok(())
}

/// Runs a file under the debugger (ending one already running), with its
/// breakpoints set before it starts. What it reports arrives as `debug` events.
pub fn debug_start(state: &IpcState, args: DebugStartArgs) -> Result<(), IpcError> {
    let path = RelPath::parse(&args.path)?;
    let root = state.trusted_root(&args.workspace_id)?;
    if !state.services.debuggers.handles(&path) {
        return Err(
            IdeError::InvalidArgument(format!("{path} can't be run under the debugger")).into(),
        );
    }
    let breakpoints = args
        .breakpoints
        .iter()
        .map(|file| Ok((RelPath::parse(&file.path)?, file.lines.clone())))
        .collect::<Result<Vec<_>, IpcError>>()?;
    state.stop_debugging();
    let emit = Arc::clone(&state.emit);
    let id = args.workspace_id.clone();
    let sink: ide_core::debug::DebugSink = Arc::new(move |event: DebugEvent| {
        emit(BackendEventDto::Debug {
            workspace_id: id.clone(),
            event: DebugEventDto::from(event),
        });
    });
    let session: Arc<dyn DebugSession> = Arc::from(state.services.debuggers.launch(
        &root,
        &path,
        &breakpoints,
        sink,
    )?);
    // A workspace opened meanwhile: this program mustn't outlive it.
    let mut debug = state.debug.lock().unwrap_or_else(PoisonError::into_inner);
    if state
        .current()
        .is_none_or(|open| open.id != args.workspace_id)
    {
        drop(debug);
        drop(session);
        return Err(IdeError::WorkspaceChanged.into());
    }
    *debug = Some(DebugRun {
        workspace_id: args.workspace_id,
        session,
    });
    Ok(())
}

/// Ends the program and the debugger.
pub fn debug_stop(state: &IpcState, args: WorkspaceArgs) -> Result<(), IpcError> {
    state.root_for(&args.workspace_id)?;
    state.stop_debugging();
    Ok(())
}

pub fn debug_breakpoints(
    state: &IpcState,
    args: DebugBreakpointsArgs,
) -> Result<Vec<BreakpointDto>, IpcError> {
    let path = RelPath::parse(&args.path)?;
    let session = state.debugging(&args.workspace_id)?;
    Ok(session
        .set_breakpoints(&path, &args.lines)?
        .into_iter()
        .map(Into::into)
        .collect())
}

/// Continues, or steps when `step` says how.
pub fn debug_resume(state: &IpcState, args: DebugThreadArgs) -> Result<(), IpcError> {
    let session = state.debugging(&args.workspace_id)?;
    match args.step {
        Some(step) => session.step(args.thread_id, step.into())?,
        None => session.resume(args.thread_id)?,
    }
    Ok(())
}

pub fn debug_pause(state: &IpcState, args: DebugThreadArgs) -> Result<(), IpcError> {
    Ok(state.debugging(&args.workspace_id)?.pause(args.thread_id)?)
}

pub fn debug_stack(
    state: &IpcState,
    args: DebugThreadArgs,
) -> Result<Vec<StackFrameDto>, IpcError> {
    Ok(state
        .debugging(&args.workspace_id)?
        .stack(args.thread_id)?
        .into_iter()
        .map(Into::into)
        .collect())
}

/// The scopes of the frame `reference`.
pub fn debug_scopes(state: &IpcState, args: DebugReferenceArgs) -> Result<Vec<ScopeDto>, IpcError> {
    Ok(state
        .debugging(&args.workspace_id)?
        .scopes(args.reference)?
        .into_iter()
        .map(Into::into)
        .collect())
}

pub fn debug_variables(
    state: &IpcState,
    args: DebugReferenceArgs,
) -> Result<Vec<VariableDto>, IpcError> {
    Ok(state
        .debugging(&args.workspace_id)?
        .variables(args.reference)?
        .into_iter()
        .map(Into::into)
        .collect())
}

pub fn debug_evaluate(state: &IpcState, args: DebugEvaluateArgs) -> Result<String, IpcError> {
    Ok(state
        .debugging(&args.workspace_id)?
        .evaluate(&args.expression, args.frame_id)?)
}

/// The MCP server for AI agents: its settings, token, and whether it runs.
pub fn agent_status(state: &IpcState) -> Result<AgentStatusDto, IpcError> {
    Ok(state.agent_status()?)
}

/// Saves the MCP server's settings, and starts, stops or restarts it to match.
pub fn agent_configure(
    state: &IpcState,
    args: AgentConfigArgs,
) -> Result<AgentStatusDto, IpcError> {
    let config = AgentConfig::from(args);
    state.services.agents.save_config(&config)?;
    state.apply_agents(&config);
    Ok(state.agent_status()?)
}

/// A new token for the MCP server; agents with the old one are refused from now on.
pub fn agent_new_token(state: &IpcState) -> Result<AgentStatusDto, IpcError> {
    state.services.agents.new_token()?;
    let config = state.services.agents.config()?;
    state.apply_agents(&config);
    Ok(state.agent_status()?)
}

/// The most of a message, and of a conversation's id, that's taken.
const MAX_MESSAGE_CHARS: usize = 200_000;
const MAX_CONVERSATION_ID: usize = 100;

fn conversation_id(id: &str) -> Result<&str, IpcError> {
    let valid = !id.is_empty()
        && id.len() <= MAX_CONVERSATION_ID
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if valid {
        Ok(id)
    } else {
        Err(IdeError::InvalidArgument("not a conversation's id".to_owned()).into())
    }
}

/// The assistant's setup, and whether a key is kept for it.
pub fn assistant_status(state: &IpcState) -> Result<AssistantStatusDto, IpcError> {
    Ok(state.services.assistant.status()?.into())
}

/// Saves the setup, and the key typed in it (into the system's credential store).
pub fn assistant_save(
    state: &IpcState,
    args: AssistantSetupArgs,
) -> Result<AssistantStatusDto, IpcError> {
    let setup = args.setup.into();
    Ok(state
        .services
        .assistant
        .save(&setup, args.secret.as_deref())?
        .into())
}

/// Forgets the setup and every key kept.
pub fn assistant_forget(state: &IpcState) -> Result<AssistantStatusDto, IpcError> {
    Ok(state.services.assistant.forget()?.into())
}

/// The models the provider offers, for the setup to choose from.
pub fn assistant_models(
    state: &IpcState,
    args: AssistantSetupArgs,
) -> Result<Vec<ModelInfoDto>, IpcError> {
    let setup = args.setup.into();
    Ok(state
        .services
        .assistant
        .list_models(&setup, args.secret.as_deref())?
        .into_iter()
        .map(Into::into)
        .collect())
}

/// Asks the model for a word: the setup works when it answers.
pub fn assistant_check(state: &IpcState, args: AssistantSetupArgs) -> Result<String, IpcError> {
    let setup = args.setup.into();
    Ok(state
        .services
        .assistant
        .check(&setup, args.secret.as_deref())?)
}

/// Sends the person's message; the reply comes as `assistant` events.
pub fn assistant_send(state: &IpcState, args: AssistantSendArgs) -> Result<(), IpcError> {
    let id = conversation_id(&args.conversation_id)?;
    if args.text.chars().count() > MAX_MESSAGE_CHARS {
        return Err(IdeError::InvalidArgument(format!(
            "a message can be at most {MAX_MESSAGE_CHARS} characters"
        ))
        .into());
    }
    let workspace = state
        .open
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .map(|open| open.root.name().to_owned());
    let context = ChatContext {
        workspace,
        active_path: args.active_path.filter(|path| !path.is_empty()),
    };
    let tools = state.services.agents.workspace_tools(state.workbench());
    let emit = Arc::clone(&state.emit);
    let sink: AssistantSink = Arc::new(move |event| {
        emit(BackendEventDto::Assistant {
            event: event.into(),
        });
    });
    Ok(state
        .services
        .assistant
        .send(id, &args.text, context, tools, sink)?)
}

/// Stops the reply coming in a conversation.
pub fn assistant_stop(state: &IpcState, args: AssistantConversationArgs) -> Result<(), IpcError> {
    state
        .services
        .assistant
        .stop(conversation_id(&args.conversation_id)?);
    Ok(())
}

/// Accepts (writes) or rejects a change the assistant proposed.
pub fn assistant_decide(state: &IpcState, args: AssistantDecideArgs) -> Result<(), IpcError> {
    Ok(state.services.assistant.decide(
        conversation_id(&args.conversation_id)?,
        &args.call_id,
        args.accept,
    )?)
}

/// Forgets a conversation.
pub fn assistant_clear(state: &IpcState, args: AssistantConversationArgs) -> Result<(), IpcError> {
    state
        .services
        .assistant
        .clear(conversation_id(&args.conversation_id)?);
    Ok(())
}
