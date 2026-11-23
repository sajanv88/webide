use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ide_core::agent::{AgentCall, AgentConfig, AgentHost, AgentServer, AgentSink, AgentWorkbench};
use ide_core::debug::{
    Breakpoint, DebugAdapters, DebugEvent, DebugSession, DebugSink, Scope, StackFrame, Step,
    Variable,
};
use ide_core::fs::{
    DirListing, FileNode, FileStamp, LineEnding, NodeKind, ReadLimits, ReadOutcome,
};
use ide_core::git::{
    DiffSide, DiffTexts, GitBranch, GitChange, GitFileStatus, GitHead, GitState, GitStatus,
};
use ide_core::language::{
    Completions, Definition, HoverText, LanguageServer, LanguageServers, LanguageSink,
    LanguageStatus, Problem, Severity, TextPosition, TextRange,
};
use ide_core::search::line_match;
use ide_core::terminal::{MAX_TERMINALS, MAX_WRITE_BYTES};
use ide_core::{
    CancelFlag, FileList, FileMatches, FsChange, FsWatcher, GitClient, IdeError, RecentWorkspace,
    RelPath, Settings, SettingsPatch, TerminalEvent, TerminalHost, TerminalSession, TerminalSink,
    TerminalSize, TextQuery, TextSearchOutcome, WatchEvent, WatchGuard, WatchSink, WorkspaceFs,
    WorkspaceRoot, WorkspaceSearch, WorkspaceStore,
};
use serde_json::json;

use crate::dto::{
    AgentConfigArgs, AppInfoDto, BackendEventDto, CloseGuardArgs, DebugStartArgs, DebugThreadArgs,
    DiffSideDto, FileBreakpoints, GitCommitArgs, GitDiffArgs, GitPathsArgs, GitSwitchArgs,
    LspDocumentArgs, LspPositionArgs, OpenRecentArgs, ReadFileDto, SearchTextArgs,
    SettingsPatchDto, StepDto, TerminalIdArgs, TerminalResizeArgs, TerminalWriteArgs,
    WorkspaceArgs, WorkspacePathArgs, WriteFileArgs,
};
use crate::error::{IpcError, IpcErrorCode};
use crate::handlers::{self, IpcState, Services};

/// Records every call; returns canned results.
#[derive(Default)]
struct FakeFs {
    calls: Mutex<Vec<String>>,
    limits_seen: Mutex<Option<ReadLimits>>,
}

impl WorkspaceFs for FakeFs {
    fn list_dir(&self, root: &WorkspaceRoot, dir: &RelPath) -> Result<DirListing, IdeError> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("list {} {dir}", root.name()));
        Ok(DirListing {
            path: dir.clone(),
            entries: vec![FileNode {
                name: "main.ts".to_owned(),
                path: dir.join("main.ts").unwrap(),
                kind: NodeKind::File,
                is_symlink: false,
                size: Some(12),
            }],
            truncated: false,
            skipped: 0,
        })
    }

    fn read_file(
        &self,
        root: &WorkspaceRoot,
        file: &RelPath,
        limits: ReadLimits,
    ) -> Result<ReadOutcome, IdeError> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("read {} {file}", root.name()));
        *self.limits_seen.lock().unwrap() = Some(limits);
        Ok(ReadOutcome::Text {
            text: "a\r\n".to_owned(),
            stamp: FileStamp {
                size: 3,
                modified_ms: Some(7),
            },
            line_ending: LineEnding::Crlf,
            read_only: None,
        })
    }

    fn write_file(
        &self,
        root: &WorkspaceRoot,
        file: &RelPath,
        text: &str,
    ) -> Result<FileStamp, IdeError> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("write {} {file} {text}", root.name()));
        Ok(FileStamp {
            size: text.len() as u64,
            modified_ms: None,
        })
    }
}

/// Opens any absolute path; the recent list is fixed.
struct FakeStore {
    recent: Vec<RecentWorkspace>,
    opened: Mutex<Vec<PathBuf>>,
}

impl WorkspaceStore for FakeStore {
    fn open(&self, path: &Path) -> Result<WorkspaceRoot, IdeError> {
        self.opened.lock().unwrap().push(path.to_path_buf());
        WorkspaceRoot::from_canonical(path.to_path_buf())
    }
    fn recent(&self) -> Result<Vec<RecentWorkspace>, IdeError> {
        Ok(self.recent.clone())
    }
    fn settings(&self) -> Result<Settings, IdeError> {
        Ok(Settings::default())
    }
    fn update_settings(&self, patch: &SettingsPatch) -> Result<Settings, IdeError> {
        Settings::default().apply(patch)
    }
}

/// Remembers each watch's sink, and which watches are still running.
#[derive(Default)]
struct FakeWatcher {
    sinks: Mutex<Vec<(PathBuf, WatchSink)>>,
    running: Arc<Mutex<Vec<PathBuf>>>,
}

/// Takes its root off the running list when dropped.
struct Running(Arc<Mutex<Vec<PathBuf>>>, PathBuf);

impl Drop for Running {
    fn drop(&mut self) {
        self.0.lock().unwrap().retain(|root| root != &self.1);
    }
}

impl FsWatcher for FakeWatcher {
    fn watch(&self, root: &WorkspaceRoot, sink: WatchSink) -> WatchGuard {
        let path = root.path().to_path_buf();
        self.sinks.lock().unwrap().push((path.clone(), sink));
        self.running.lock().unwrap().push(path.clone());
        WatchGuard::new(Running(Arc::clone(&self.running), path))
    }
}

impl FakeWatcher {
    fn report(&self, index: usize, event: WatchEvent) {
        let sink = Arc::clone(&self.sinks.lock().unwrap()[index].1);
        sink(event);
    }
}

/// Records each query and its cancel flag; finds one line in `src/a.ts`.
#[derive(Default)]
struct FakeSearch {
    queries: Mutex<Vec<(TextQuery, CancelFlag)>>,
}

impl WorkspaceSearch for FakeSearch {
    fn search_text(
        &self,
        _root: &WorkspaceRoot,
        query: &TextQuery,
        cancel: &CancelFlag,
    ) -> Result<TextSearchOutcome, IdeError> {
        self.queries
            .lock()
            .unwrap()
            .push((query.clone(), cancel.clone()));
        Ok(TextSearchOutcome {
            files: vec![FileMatches {
                path: RelPath::parse("src/a.ts").unwrap(),
                lines: vec![line_match(2, "let needle = 1;", &[(4, 10)])],
            }],
            truncated: false,
            cancelled: false,
        })
    }

    fn list_files(
        &self,
        _root: &WorkspaceRoot,
        _cancel: &CancelFlag,
    ) -> Result<FileList, IdeError> {
        Ok(FileList {
            files: vec![RelPath::parse("src/a.ts").unwrap()],
            truncated: false,
        })
    }
}

/// Records each call; one modified file on `main`.
#[derive(Default)]
struct FakeGit {
    calls: Mutex<Vec<String>>,
}

impl FakeGit {
    fn record(&self, call: String) {
        self.calls.lock().unwrap().push(call);
    }
}

fn joined(paths: &[RelPath]) -> String {
    paths
        .iter()
        .map(RelPath::as_str)
        .collect::<Vec<_>>()
        .join(",")
}

impl GitClient for FakeGit {
    fn state(&self, _root: &WorkspaceRoot) -> Result<GitState, IdeError> {
        Ok(GitState::Repository(GitStatus {
            head: GitHead {
                branch: Some("main".into()),
                commit: Some("abc1234".into()),
                upstream: None,
                ahead: 0,
                behind: 0,
            },
            files: vec![GitFileStatus {
                path: RelPath::parse("src/a.ts").unwrap(),
                from: None,
                staged: None,
                unstaged: Some(GitChange::Modified),
            }],
            truncated: false,
        }))
    }
    fn diff_texts(
        &self,
        _root: &WorkspaceRoot,
        path: &RelPath,
        side: DiffSide,
        limit: u64,
    ) -> Result<DiffTexts, IdeError> {
        self.record(format!("diff {path} {side:?} {limit}"));
        Ok(DiffTexts::Text {
            original: "a".into(),
            modified: "b".into(),
        })
    }
    fn stage(&self, _root: &WorkspaceRoot, paths: &[RelPath]) -> Result<(), IdeError> {
        self.record(format!("stage {}", joined(paths)));
        Ok(())
    }
    fn unstage(&self, _root: &WorkspaceRoot, paths: &[RelPath]) -> Result<(), IdeError> {
        self.record(format!("unstage {}", joined(paths)));
        Ok(())
    }
    fn discard(&self, _root: &WorkspaceRoot, paths: &[RelPath]) -> Result<(), IdeError> {
        self.record(format!("discard {}", joined(paths)));
        Ok(())
    }
    fn commit(&self, _root: &WorkspaceRoot, message: &str) -> Result<(), IdeError> {
        self.record(format!("commit {message}"));
        Err(IdeError::Git("nothing to commit".into()))
    }
    fn branches(&self, _root: &WorkspaceRoot) -> Result<Vec<GitBranch>, IdeError> {
        Ok(vec![GitBranch {
            name: "main".into(),
            current: true,
        }])
    }
    fn switch_branch(
        &self,
        _root: &WorkspaceRoot,
        name: &str,
        create: bool,
    ) -> Result<(), IdeError> {
        self.record(format!("switch {name} {create}"));
        Ok(())
    }
    fn init(&self, _root: &WorkspaceRoot) -> Result<(), IdeError> {
        self.record("init".into());
        Ok(())
    }
}

/// Records what happens to each terminal; says hello on start.
#[derive(Default)]
struct FakeTerminals {
    log: Arc<Mutex<Vec<String>>>,
}

struct FakeSession {
    n: usize,
    log: Arc<Mutex<Vec<String>>>,
}

impl TerminalHost for FakeTerminals {
    fn spawn(
        &self,
        cwd: &Path,
        size: TerminalSize,
        sink: TerminalSink,
    ) -> Result<Box<dyn TerminalSession>, IdeError> {
        let mut log = self.log.lock().unwrap();
        let n = log.iter().filter(|line| line.starts_with("spawn")).count() + 1;
        log.push(format!(
            "spawn {n} in {} at {}x{}",
            cwd.file_name().unwrap().to_string_lossy(),
            size.cols,
            size.rows
        ));
        sink(TerminalEvent::Output("hello".into()));
        Ok(Box::new(FakeSession {
            n,
            log: Arc::clone(&self.log),
        }))
    }
}

impl TerminalSession for FakeSession {
    fn write(&self, data: &[u8]) -> Result<(), IdeError> {
        self.log.lock().unwrap().push(format!(
            "write {} {}",
            self.n,
            String::from_utf8_lossy(data)
        ));
        Ok(())
    }
    fn resize(&self, size: TerminalSize) -> Result<(), IdeError> {
        self.log
            .lock()
            .unwrap()
            .push(format!("resize {} {}x{}", self.n, size.cols, size.rows));
        Ok(())
    }
    fn shell(&self) -> &str {
        "fakesh"
    }
}

impl Drop for FakeSession {
    fn drop(&mut self) {
        self.log.lock().unwrap().push(format!("end {}", self.n));
    }
}

/// Starts once per call; logs what each server is told; can be made to fail.
#[derive(Default)]
struct FakeLanguages {
    log: Arc<Mutex<Vec<String>>>,
    fail: std::sync::atomic::AtomicBool,
}

struct FakeServer {
    n: usize,
    log: Arc<Mutex<Vec<String>>>,
}

impl LanguageServers for FakeLanguages {
    fn handles(&self, path: &RelPath) -> bool {
        path.as_str().ends_with(".ts")
    }
    fn start(
        &self,
        root: &WorkspaceRoot,
        status: LanguageSink,
    ) -> Result<Box<dyn LanguageServer>, IdeError> {
        let mut log = self.log.lock().unwrap();
        let n = log.iter().filter(|line| line.starts_with("start")).count() + 1;
        log.push(format!("start {n} in {}", root.name()));
        status(LanguageStatus::Starting);
        if self.fail.load(std::sync::atomic::Ordering::Relaxed) {
            status(LanguageStatus::Failed("no server".into()));
            return Err(IdeError::Io("no server".into()));
        }
        status(LanguageStatus::Ready);
        Ok(Box::new(FakeServer {
            n,
            log: Arc::clone(&self.log),
        }))
    }
}

impl FakeServer {
    fn note(&self, what: String) {
        self.log.lock().unwrap().push(format!("{} {what}", self.n));
    }
}

fn at(line: u32, character: u32) -> TextPosition {
    TextPosition { line, character }
}

impl LanguageServer for FakeServer {
    fn open(&self, path: &RelPath, text: &str) -> Result<(), IdeError> {
        self.note(format!("open {path} {text}"));
        Ok(())
    }
    fn change(&self, path: &RelPath, text: &str) -> Result<(), IdeError> {
        self.note(format!("change {path} {text}"));
        Ok(())
    }
    fn close(&self, path: &RelPath) -> Result<(), IdeError> {
        self.note(format!("close {path}"));
        Ok(())
    }
    fn problems(&self, _path: &RelPath) -> Result<Vec<Problem>, IdeError> {
        Ok(vec![Problem {
            range: TextRange {
                start: at(0, 6),
                end: at(0, 7),
            },
            severity: Severity::Error,
            message: "Type 'string' is not assignable to type 'number'.".into(),
            source: Some("ts".into()),
            code: Some("2322".into()),
        }])
    }
    fn completions(&self, _path: &RelPath, _at: TextPosition) -> Result<Completions, IdeError> {
        Ok(Completions::default())
    }
    fn hover(&self, _path: &RelPath, _at: TextPosition) -> Result<Option<HoverText>, IdeError> {
        Ok(Some(HoverText {
            markdown: "const x: number".into(),
        }))
    }
    fn definition(&self, path: &RelPath, _at: TextPosition) -> Result<Vec<Definition>, IdeError> {
        Ok(vec![Definition::Workspace {
            path: path.clone(),
            range: TextRange {
                start: at(0, 6),
                end: at(0, 7),
            },
        }])
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.note("stop".into());
    }
}

/// Logs what each debug session is told; reports a stop when launched.
#[derive(Default)]
struct FakeDebuggers {
    log: Arc<Mutex<Vec<String>>>,
}

struct FakeDebugSession {
    log: Arc<Mutex<Vec<String>>>,
}

impl DebugAdapters for FakeDebuggers {
    fn handles(&self, path: &RelPath) -> bool {
        path.as_str().ends_with(".js")
    }
    fn launch(
        &self,
        root: &WorkspaceRoot,
        program: &RelPath,
        breakpoints: &[(RelPath, Vec<u32>)],
        sink: DebugSink,
    ) -> Result<Box<dyn DebugSession>, IdeError> {
        self.log.lock().unwrap().push(format!(
            "launch {program} in {} with {breakpoints:?}",
            root.name()
        ));
        sink(DebugEvent::Stopped {
            thread_id: 7,
            reason: "breakpoint".into(),
            description: None,
        });
        Ok(Box::new(FakeDebugSession {
            log: Arc::clone(&self.log),
        }))
    }
}

impl FakeDebugSession {
    fn note(&self, what: String) {
        self.log.lock().unwrap().push(what);
    }
}

impl DebugSession for FakeDebugSession {
    fn set_breakpoints(&self, path: &RelPath, lines: &[u32]) -> Result<Vec<Breakpoint>, IdeError> {
        self.note(format!("breakpoints {path} {lines:?}"));
        Ok(lines
            .iter()
            .map(|&line| Breakpoint {
                line,
                verified: true,
                actual_line: None,
            })
            .collect())
    }
    fn resume(&self, thread_id: i64) -> Result<(), IdeError> {
        self.note(format!("resume {thread_id}"));
        Ok(())
    }
    fn step(&self, thread_id: i64, step: Step) -> Result<(), IdeError> {
        self.note(format!("step {thread_id} {step:?}"));
        Ok(())
    }
    fn pause(&self, thread_id: i64) -> Result<(), IdeError> {
        self.note(format!("pause {thread_id}"));
        Ok(())
    }
    fn stack(&self, _thread_id: i64) -> Result<Vec<StackFrame>, IdeError> {
        Ok(Vec::new())
    }
    fn scopes(&self, _frame_id: i64) -> Result<Vec<Scope>, IdeError> {
        Ok(Vec::new())
    }
    fn variables(&self, _reference: i64) -> Result<Vec<Variable>, IdeError> {
        Ok(Vec::new())
    }
    fn evaluate(&self, expression: &str, _frame_id: Option<i64>) -> Result<String, IdeError> {
        Ok(format!("= {expression}"))
    }
}

impl Drop for FakeDebugSession {
    fn drop(&mut self) {
        self.note("end".into());
    }
}

/// Keeps a configuration and token in memory; "serves" by logging, and
/// refuses port 1999 as if it were taken.
struct FakeAgents {
    config: Mutex<AgentConfig>,
    token: Mutex<String>,
    log: Arc<Mutex<Vec<String>>>,
    workbench: Mutex<Option<(Arc<dyn AgentWorkbench>, AgentSink)>>,
}

impl Default for FakeAgents {
    fn default() -> Self {
        Self {
            config: Mutex::new(AgentConfig::default()),
            token: Mutex::new("t1".to_owned()),
            log: Arc::default(),
            workbench: Mutex::new(None),
        }
    }
}

struct FakeAgentServer {
    port: u16,
    log: Arc<Mutex<Vec<String>>>,
}

impl AgentHost for FakeAgents {
    fn config(&self) -> Result<AgentConfig, IdeError> {
        Ok(*self.config.lock().unwrap())
    }
    fn save_config(&self, config: &AgentConfig) -> Result<(), IdeError> {
        config.validate()?;
        *self.config.lock().unwrap() = *config;
        Ok(())
    }
    fn token(&self) -> Result<String, IdeError> {
        Ok(self.token.lock().unwrap().clone())
    }
    fn new_token(&self) -> Result<String, IdeError> {
        let mut token = self.token.lock().unwrap();
        *token = format!("{token}+");
        Ok(token.clone())
    }
    fn serve(
        &self,
        config: &AgentConfig,
        token: &str,
        workbench: Arc<dyn AgentWorkbench>,
        sink: AgentSink,
    ) -> Result<Box<dyn AgentServer>, IdeError> {
        if config.port == 1999 {
            return Err(IdeError::Io("port 1999 is in use".to_owned()));
        }
        self.log.lock().unwrap().push(format!(
            "serve {} edits={} token={token}",
            config.port, config.allow_edits
        ));
        *self.workbench.lock().unwrap() = Some((workbench, sink));
        Ok(Box::new(FakeAgentServer {
            port: config.port,
            log: Arc::clone(&self.log),
        }))
    }
}

impl AgentServer for FakeAgentServer {
    fn url(&self) -> String {
        format!("http://127.0.0.1:{}/mcp", self.port)
    }
}

impl Drop for FakeAgentServer {
    fn drop(&mut self) {
        self.log.lock().unwrap().push(format!("stop {}", self.port));
    }
}

fn folder(name: &str) -> PathBuf {
    std::env::temp_dir().join(name)
}

struct Fixture {
    state: IpcState,
    fs: Arc<FakeFs>,
    store: Arc<FakeStore>,
    watcher: Arc<FakeWatcher>,
    search: Arc<FakeSearch>,
    git: Arc<FakeGit>,
    terminals: Arc<FakeTerminals>,
    languages: Arc<FakeLanguages>,
    debuggers: Arc<FakeDebuggers>,
    agents: Arc<FakeAgents>,
    /// What was sent to the webview.
    emitted: Arc<Mutex<Vec<BackendEventDto>>>,
}

fn fixture() -> Fixture {
    let fs = Arc::new(FakeFs::default());
    let store = Arc::new(FakeStore {
        recent: vec![
            RecentWorkspace {
                path: folder("alpha"),
                name: "alpha".to_owned(),
                opened_at_ms: 2,
            },
            RecentWorkspace {
                path: folder("beta"),
                name: "beta".to_owned(),
                opened_at_ms: 1,
            },
        ],
        opened: Mutex::new(Vec::new()),
    });
    let watcher = Arc::new(FakeWatcher::default());
    let search = Arc::new(FakeSearch::default());
    let git = Arc::new(FakeGit::default());
    let terminals = Arc::new(FakeTerminals::default());
    let languages = Arc::new(FakeLanguages::default());
    let debuggers = Arc::new(FakeDebuggers::default());
    let agents = Arc::new(FakeAgents::default());
    let services = Services {
        fs: fs.clone(),
        workspace: store.clone(),
        watcher: watcher.clone(),
        search: search.clone(),
        git: git.clone(),
        terminals: terminals.clone(),
        languages: languages.clone(),
        debuggers: debuggers.clone(),
        agents: agents.clone(),
    };
    let info = AppInfoDto {
        name: "test".to_owned(),
        version: "0.0.0".to_owned(),
    };
    let emitted = Arc::new(Mutex::new(Vec::new()));
    let sent = Arc::clone(&emitted);
    let emit: crate::handlers::Emit = Arc::new(move |event| sent.lock().unwrap().push(event));
    Fixture {
        state: IpcState::new(services, info, emit),
        fs,
        store,
        watcher,
        search,
        git,
        terminals,
        languages,
        debuggers,
        agents,
        emitted,
    }
}

fn search_args(workspace_id: &str, pattern: &str) -> SearchTextArgs {
    SearchTextArgs {
        workspace_id: workspace_id.to_owned(),
        pattern: pattern.to_owned(),
        is_regex: false,
        match_case: true,
        whole_word: false,
        include: vec!["*.ts".to_owned()],
        exclude: Vec::new(),
    }
}

fn args(workspace_id: &str, path: &str) -> WorkspacePathArgs {
    WorkspacePathArgs {
        workspace_id: workspace_id.to_owned(),
        path: path.to_owned(),
    }
}

fn code<T: std::fmt::Debug>(result: Result<T, IpcError>) -> IpcErrorCode {
    match result {
        Err(error) => error.code,
        Ok(value) => panic!("expected an error, got {value:?}"),
    }
}

#[test]
fn file_commands_need_the_open_workspace() {
    let fx = fixture();
    assert_eq!(
        code(handlers::list_directory(&fx.state, args("1", ""))),
        IpcErrorCode::NoWorkspace
    );

    let alpha = handlers::open_picked(&fx.state, &folder("alpha")).unwrap();
    assert!(handlers::list_directory(&fx.state, args(&alpha.id, "")).is_ok());

    let beta = handlers::open_picked(&fx.state, &folder("beta")).unwrap();
    assert_ne!(alpha.id, beta.id);
    let stale = handlers::write_file(
        &fx.state,
        WriteFileArgs {
            workspace_id: alpha.id,
            path: "a.txt".to_owned(),
            text: "x".to_owned(),
        },
    );
    assert_eq!(code(stale), IpcErrorCode::WorkspaceChanged);
    assert!(
        !fx.fs
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|call| call.starts_with("write"))
    );
}

#[test]
fn invalid_paths_never_reach_the_file_system() {
    let fx = fixture();
    let ws = handlers::open_picked(&fx.state, &folder("alpha")).unwrap();
    for path in ["..", "../x", "a/../../b", "/etc/passwd", "a//b", "./a"] {
        assert_eq!(
            code(handlers::read_file(&fx.state, args(&ws.id, path))),
            IpcErrorCode::InvalidPath,
            "{path}"
        );
        assert_eq!(
            code(handlers::list_directory(&fx.state, args(&ws.id, path))),
            IpcErrorCode::InvalidPath,
            "{path}"
        );
    }
    assert!(fx.fs.calls.lock().unwrap().is_empty());
}

#[test]
fn only_recent_folders_can_be_opened_by_path() {
    let fx = fixture();
    let outside = folder("not-recent").to_string_lossy().into_owned();
    let refused = handlers::open_recent_workspace(&fx.state, OpenRecentArgs { path: outside });
    assert_eq!(code(refused), IpcErrorCode::NotARecentWorkspace);
    assert!(fx.store.opened.lock().unwrap().is_empty());

    let recent = handlers::recent_workspaces(&fx.state).unwrap();
    assert_eq!(
        recent.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        ["alpha", "beta"]
    );
    let opened = handlers::open_recent_workspace(
        &fx.state,
        OpenRecentArgs {
            path: recent[1].path.clone(),
        },
    )
    .unwrap();
    assert_eq!(opened.name, "beta");
    assert_eq!(
        handlers::current_workspace(&fx.state).unwrap(),
        Some(opened)
    );
}

#[test]
fn reads_use_the_size_limits_from_settings() {
    let fx = fixture();
    let ws = handlers::open_picked(&fx.state, &folder("alpha")).unwrap();
    handlers::read_file(&fx.state, args(&ws.id, "src/main.ts")).unwrap();
    assert_eq!(
        *fx.fs.limits_seen.lock().unwrap(),
        Some(Settings::default().read_limits())
    );
}

#[test]
fn dtos_have_the_shape_the_frontend_expects() {
    let fx = fixture();
    let ws = handlers::open_picked(&fx.state, &folder("alpha")).unwrap();

    let read = handlers::read_file(&fx.state, args(&ws.id, "src/main.ts")).unwrap();
    assert!(matches!(read, ReadFileDto::Text(_)));
    assert_eq!(
        serde_json::to_value(&read).unwrap(),
        json!({
            "kind": "text",
            "path": "src/main.ts",
            "text": "a\r\n",
            "stamp": { "size": 3, "modifiedMs": 7 },
            "lineEnding": "crlf",
            "readOnly": null
        })
    );

    let listing = handlers::list_directory(&fx.state, args(&ws.id, "src")).unwrap();
    assert_eq!(
        serde_json::to_value(&listing).unwrap(),
        json!({
            "path": "src",
            "entries": [{ "name": "main.ts", "path": "src/main.ts", "kind": "file", "isSymlink": false, "size": 12 }],
            "truncated": false,
            "skipped": 0
        })
    );

    let error = IpcError::from(IdeError::OutsideWorkspace);
    assert_eq!(
        serde_json::to_value(&error).unwrap(),
        json!({ "code": "outsideWorkspace", "message": "the path is outside the workspace" })
    );
}

#[test]
fn args_reject_unknown_fields_and_patches_accept_partial_input() {
    let unknown = serde_json::from_value::<WorkspacePathArgs>(
        json!({ "workspaceId": "1", "path": "", "extra": 1 }),
    );
    assert!(unknown.is_err());
    let patch: SettingsPatchDto = serde_json::from_value(json!({ "theme": "dark" })).unwrap();
    assert_eq!(patch.read_only_above_bytes, None);

    let fx = fixture();
    let updated = handlers::update_settings(&fx.state, patch).unwrap();
    assert_eq!(serde_json::to_value(updated).unwrap()["theme"], "dark");
    let invalid = handlers::update_settings(
        &fx.state,
        SettingsPatchDto {
            read_only_above_bytes: Some(1),
            ..Default::default()
        },
    );
    assert_eq!(code(invalid), IpcErrorCode::InvalidSettings);
}

#[test]
fn opening_a_workspace_watches_it_and_stops_watching_the_previous_one() {
    let fx = fixture();
    let first = handlers::open_picked(&fx.state, &folder("alpha")).unwrap();
    assert_eq!(*fx.watcher.running.lock().unwrap(), [folder("alpha")]);

    let second = handlers::open_picked(&fx.state, &folder("beta")).unwrap();
    assert_eq!(*fx.watcher.running.lock().unwrap(), [folder("beta")]);
    assert_ne!(first.id, second.id);
}

#[test]
fn watch_events_reach_the_webview_tagged_with_their_workspace() {
    let fx = fixture();
    let opened = handlers::open_picked(&fx.state, &folder("alpha")).unwrap();
    fx.watcher.report(
        0,
        WatchEvent::Changed {
            changes: vec![
                FsChange::Modified(RelPath::parse("src/a.ts").unwrap()),
                FsChange::Renamed {
                    from: RelPath::parse("old.ts").unwrap(),
                    to: RelPath::parse("new.ts").unwrap(),
                },
            ],
            rescan: false,
        },
    );
    fx.watcher.report(
        0,
        WatchEvent::Failed {
            message: "gone".to_owned(),
        },
    );
    let sent: Vec<_> = fx
        .emitted
        .lock()
        .unwrap()
        .iter()
        .map(|event| serde_json::to_value(event).unwrap())
        .collect();
    assert_eq!(
        sent,
        [
            json!({
                "kind": "filesChanged",
                "workspaceId": opened.id,
                "changes": [
                    { "kind": "modified", "path": "src/a.ts" },
                    { "kind": "renamed", "from": "old.ts", "path": "new.ts" },
                ],
                "rescan": false,
            }),
            json!({ "kind": "watchFailed", "workspaceId": opened.id, "message": "gone" }),
        ]
    );
    assert_eq!(
        serde_json::to_value(BackendEventDto::CloseRequested).unwrap(),
        json!({ "kind": "closeRequested" })
    );
}

#[test]
fn the_close_guard_holds_the_window_only_while_a_page_can_answer() {
    use tauri::ipc::{Channel, InvokeResponseBody};

    let hub = crate::events::EventHub::default();
    // Nothing unsaved: closes.
    assert!(!hub.hold_close());

    // Unsaved, but no page listening: nothing could answer, so it closes.
    hub.set_unsaved(true);
    assert!(!hub.hold_close());

    let received = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = Arc::clone(&received);
    hub.subscribe(Channel::new(move |body: InvokeResponseBody| {
        if let InvokeResponseBody::Json(json) = body {
            sink.lock().unwrap().push(json);
        }
        Ok(())
    }));
    // Subscribing starts from nothing unsaved.
    assert!(!hub.hold_close());

    let args: CloseGuardArgs = serde_json::from_value(json!({ "unsaved": true })).unwrap();
    hub.set_unsaved(args.unsaved);
    assert!(hub.hold_close());
    assert_eq!(*received.lock().unwrap(), [r#"{"kind":"closeRequested"}"#]);

    // The page closes the window itself, after asking.
    hub.release_close();
    assert!(!hub.hold_close());

    // A reload forgets the old page.
    hub.set_unsaved(true);
    hub.reset();
    assert!(!hub.hold_close());
}

#[test]
fn search_needs_the_open_workspace_and_passes_the_query_through() {
    let fx = fixture();
    assert_eq!(
        code(handlers::search_text(&fx.state, search_args("1", "needle"))),
        IpcErrorCode::NoWorkspace
    );
    let ws = handlers::open_picked(&fx.state, &folder("alpha")).unwrap();
    let found = handlers::search_text(&fx.state, search_args(&ws.id, "needle")).unwrap();
    assert_eq!(
        serde_json::to_value(&found).unwrap(),
        json!({
            "files": [{
                "path": "src/a.ts",
                "lines": [{
                    "line": 2,
                    "column": 5,
                    "preview": "let needle = 1;",
                    "spans": [{ "start": 4, "end": 10 }],
                }],
            }],
            "truncated": false,
            "cancelled": false,
        })
    );
    let queries = fx.search.queries.lock().unwrap();
    let (query, _) = &queries[0];
    assert_eq!(query.pattern, "needle");
    assert!(query.match_case);
    assert_eq!(query.include, ["*.ts"]);
    // Files too large to open aren't searched either.
    assert_eq!(
        query.max_file_bytes,
        Settings::default().read_limits().refuse_above
    );
}

#[test]
fn a_new_search_or_workspace_cancels_the_running_search() {
    let fx = fixture();
    let ws = handlers::open_picked(&fx.state, &folder("alpha")).unwrap();
    handlers::search_text(&fx.state, search_args(&ws.id, "one")).unwrap();
    handlers::search_text(&fx.state, search_args(&ws.id, "two")).unwrap();
    {
        let queries = fx.search.queries.lock().unwrap();
        assert!(queries[0].1.is_cancelled());
        assert!(!queries[1].1.is_cancelled());
    }
    handlers::open_picked(&fx.state, &folder("beta")).unwrap();
    assert!(fx.search.queries.lock().unwrap()[1].1.is_cancelled());
}

#[test]
fn list_files_returns_workspace_relative_paths() {
    let fx = fixture();
    let ws = handlers::open_picked(&fx.state, &folder("alpha")).unwrap();
    let args = WorkspaceArgs {
        workspace_id: ws.id,
    };
    let list = handlers::list_files(&fx.state, args).unwrap();
    assert_eq!(
        serde_json::to_value(&list).unwrap(),
        json!({ "files": ["src/a.ts"], "truncated": false })
    );
}

#[test]
fn git_commands_need_the_open_workspace_and_valid_paths() {
    let fx = fixture();
    let ws_args = |id: &str| WorkspaceArgs {
        workspace_id: id.to_owned(),
    };
    assert_eq!(
        code(handlers::git_state(&fx.state, ws_args("1"))),
        IpcErrorCode::NoWorkspace
    );
    let ws = handlers::open_picked(&fx.state, &folder("alpha")).unwrap();

    // A path that could leave the workspace never reaches git.
    let bad = GitPathsArgs {
        workspace_id: ws.id.clone(),
        paths: vec!["src/a.ts".into(), "../outside".into()],
    };
    assert_eq!(
        code(handlers::git_stage(&fx.state, bad)),
        IpcErrorCode::InvalidPath
    );
    assert!(fx.git.calls.lock().unwrap().is_empty());

    let good = GitPathsArgs {
        workspace_id: ws.id.clone(),
        paths: vec!["src/a.ts".into(), "b.ts".into()],
    };
    handlers::git_stage(&fx.state, good.clone()).unwrap();
    handlers::git_unstage(&fx.state, good.clone()).unwrap();
    handlers::git_discard(&fx.state, good).unwrap();
    handlers::git_switch_branch(
        &fx.state,
        GitSwitchArgs {
            workspace_id: ws.id.clone(),
            name: "feature".into(),
            create: true,
        },
    )
    .unwrap();
    // Git's refusal reaches the page with the git code and git's words.
    let refused = handlers::git_commit(
        &fx.state,
        GitCommitArgs {
            workspace_id: ws.id.clone(),
            message: "first".into(),
        },
    )
    .unwrap_err();
    assert_eq!(refused.code, IpcErrorCode::Git);
    assert_eq!(refused.message, "nothing to commit");
    handlers::git_diff(
        &fx.state,
        GitDiffArgs {
            workspace_id: ws.id.clone(),
            path: "src/a.ts".into(),
            side: DiffSideDto::Unstaged,
        },
    )
    .unwrap();
    assert_eq!(
        *fx.git.calls.lock().unwrap(),
        [
            "stage src/a.ts,b.ts".to_owned(),
            "unstage src/a.ts,b.ts".to_owned(),
            "discard src/a.ts,b.ts".to_owned(),
            "switch feature true".to_owned(),
            "commit first".to_owned(),
            // The limit is the "don't open above" size.
            format!(
                "diff src/a.ts Unstaged {}",
                Settings::default().read_limits().refuse_above
            ),
        ]
    );
}

#[test]
fn git_state_has_the_shape_the_frontend_expects() {
    let fx = fixture();
    let ws = handlers::open_picked(&fx.state, &folder("alpha")).unwrap();
    let state = handlers::git_state(
        &fx.state,
        WorkspaceArgs {
            workspace_id: ws.id.clone(),
        },
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&state).unwrap(),
        json!({
            "kind": "repository",
            "head": { "branch": "main", "commit": "abc1234", "upstream": null, "ahead": 0, "behind": 0 },
            "files": [{ "path": "src/a.ts", "from": null, "staged": null, "unstaged": "modified" }],
            "truncated": false,
        })
    );
    let diff = handlers::git_diff(
        &fx.state,
        GitDiffArgs {
            workspace_id: ws.id,
            path: "src/a.ts".into(),
            side: DiffSideDto::Staged,
        },
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&diff).unwrap(),
        json!({ "kind": "text", "original": "a", "modified": "b" })
    );
    // Sides are camelCase strings.
    let args: GitDiffArgs =
        serde_json::from_value(json!({ "workspaceId": "1", "path": "a", "side": "unstaged" }))
            .unwrap();
    assert_eq!(args.side, DiffSideDto::Unstaged);
}

fn collect() -> (TerminalSink, Arc<Mutex<Vec<TerminalEvent>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let into = Arc::clone(&seen);
    (
        Arc::new(move |event| into.lock().unwrap().push(event)),
        seen,
    )
}

#[test]
fn terminals_open_in_the_workspace_and_take_input() {
    let fx = fixture();
    let (sink, seen) = collect();
    assert_eq!(
        code(handlers::terminal_open(
            &fx.state,
            "1",
            80,
            24,
            sink.clone()
        )),
        IpcErrorCode::NoWorkspace
    );
    let ws = handlers::open_picked(&fx.state, &folder("alpha")).unwrap();
    // A size no terminal has never reaches the system.
    assert_eq!(
        code(handlers::terminal_open(
            &fx.state,
            &ws.id,
            0,
            24,
            sink.clone()
        )),
        IpcErrorCode::InvalidArgument
    );
    let term = handlers::terminal_open(&fx.state, &ws.id, 80, 24, sink).unwrap();
    assert_eq!(term.shell, "fakesh");
    assert_eq!(
        *seen.lock().unwrap(),
        [TerminalEvent::Output("hello".into())]
    );

    handlers::terminal_write(
        &fx.state,
        TerminalWriteArgs {
            id: term.id,
            data: "ls\r".into(),
        },
    )
    .unwrap();
    handlers::terminal_resize(
        &fx.state,
        TerminalResizeArgs {
            id: term.id,
            cols: 100,
            rows: 30,
        },
    )
    .unwrap();
    assert_eq!(
        code(handlers::terminal_write(
            &fx.state,
            TerminalWriteArgs {
                id: term.id,
                data: "x".repeat(MAX_WRITE_BYTES + 1),
            },
        )),
        IpcErrorCode::InvalidArgument
    );
    handlers::terminal_close(&fx.state, TerminalIdArgs { id: term.id }).unwrap();
    assert_eq!(
        code(handlers::terminal_write(
            &fx.state,
            TerminalWriteArgs {
                id: term.id,
                data: "x".into(),
            },
        )),
        IpcErrorCode::NotFound
    );
    assert_eq!(
        *fx.terminals.log.lock().unwrap(),
        [
            "spawn 1 in alpha at 80x24",
            "write 1 ls\r",
            "resize 1 100x30",
            "end 1"
        ]
    );
}

#[test]
fn another_workspace_or_too_many_terminals_end_or_refuse_them() {
    let fx = fixture();
    let ws = handlers::open_picked(&fx.state, &folder("alpha")).unwrap();
    for _ in 0..MAX_TERMINALS {
        handlers::terminal_open(&fx.state, &ws.id, 80, 24, collect().0).unwrap();
    }
    assert_eq!(
        code(handlers::terminal_open(
            &fx.state,
            &ws.id,
            80,
            24,
            collect().0
        )),
        IpcErrorCode::InvalidArgument
    );
    // Opening another folder ends the old one's terminals.
    handlers::open_picked(&fx.state, &folder("beta")).unwrap();
    let ended = fx
        .terminals
        .log
        .lock()
        .unwrap()
        .iter()
        .filter(|line| line.starts_with("end"))
        .count();
    assert_eq!(ended, MAX_TERMINALS);
    // And the page starting again ends them too.
    let ws = handlers::current_workspace(&fx.state).unwrap().unwrap();
    handlers::terminal_open(&fx.state, &ws.id, 80, 24, collect().0).unwrap();
    fx.state.close_terminals();
    assert_eq!(
        fx.terminals.log.lock().unwrap().last().map(String::as_str),
        Some("end 11")
    );
}

fn doc(workspace_id: &str, path: &str, text: &str) -> LspDocumentArgs {
    LspDocumentArgs {
        workspace_id: workspace_id.to_owned(),
        path: path.to_owned(),
        text: text.to_owned(),
    }
}

fn place(workspace_id: &str, path: &str) -> LspPositionArgs {
    LspPositionArgs {
        workspace_id: workspace_id.to_owned(),
        path: path.to_owned(),
        line: 0,
        character: 6,
    }
}

#[test]
fn the_language_server_starts_with_the_first_file_it_handles_and_stops_with_the_workspace() {
    let fx = fixture();
    let ws = handlers::open_picked(&fx.state, &folder("alpha")).unwrap();
    // Not its kind of file: nothing starts.
    handlers::lsp_open(&fx.state, doc(&ws.id, "notes.md", "x")).unwrap();
    assert!(fx.languages.log.lock().unwrap().is_empty());

    handlers::lsp_open(&fx.state, doc(&ws.id, "a.ts", "one")).unwrap();
    handlers::lsp_change(&fx.state, doc(&ws.id, "a.ts", "two")).unwrap();
    let problems = handlers::lsp_problems(&fx.state, args(&ws.id, "a.ts")).unwrap();
    assert_eq!(
        serde_json::to_value(&problems).unwrap(),
        json!([{
            "range": { "start": { "line": 0, "character": 6 }, "end": { "line": 0, "character": 7 } },
            "severity": "error",
            "message": "Type 'string' is not assignable to type 'number'.",
            "source": "ts",
            "code": "2322",
        }])
    );
    let definition = handlers::lsp_definition(&fx.state, place(&ws.id, "a.ts")).unwrap();
    assert_eq!(
        serde_json::to_value(&definition).unwrap()[0]["kind"],
        json!("workspace")
    );
    assert!(
        handlers::lsp_hover(&fx.state, place(&ws.id, "a.ts"))
            .unwrap()
            .is_some()
    );
    handlers::lsp_close(&fx.state, args(&ws.id, "a.ts")).unwrap();
    // A path that could leave the workspace never reaches the server.
    assert_eq!(
        code(handlers::lsp_open(&fx.state, doc(&ws.id, "../x.ts", "x"))),
        IpcErrorCode::InvalidPath
    );

    // Its status reached the page, tagged with the workspace.
    let statuses: Vec<_> = fx
        .emitted
        .lock()
        .unwrap()
        .iter()
        .filter_map(|event| match event {
            BackendEventDto::LanguageStatus {
                workspace_id,
                status,
            } => Some(format!("{workspace_id} {status:?}")),
            _ => None,
        })
        .collect();
    assert_eq!(
        statuses,
        [format!("{} Starting", ws.id), format!("{} Ready", ws.id)]
    );

    handlers::open_picked(&fx.state, &folder("beta")).unwrap();
    assert_eq!(
        *fx.languages.log.lock().unwrap(),
        [
            "start 1 in alpha",
            "1 open a.ts one",
            "1 change a.ts two",
            "1 close a.ts",
            "1 stop",
        ]
    );
}

#[test]
fn a_server_that_cant_start_isnt_tried_again_until_asked() {
    let fx = fixture();
    fx.languages
        .fail
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let ws = handlers::open_picked(&fx.state, &folder("alpha")).unwrap();
    handlers::lsp_open(&fx.state, doc(&ws.id, "a.ts", "x")).unwrap();
    assert!(
        handlers::lsp_problems(&fx.state, args(&ws.id, "a.ts"))
            .unwrap()
            .is_empty()
    );
    let starts = || {
        fx.languages
            .log
            .lock()
            .unwrap()
            .iter()
            .filter(|line| line.starts_with("start"))
            .count()
    };
    assert_eq!(starts(), 1);

    fx.languages
        .fail
        .store(false, std::sync::atomic::Ordering::Relaxed);
    handlers::lsp_restart(
        &fx.state,
        WorkspaceArgs {
            workspace_id: ws.id.clone(),
        },
    )
    .unwrap();
    handlers::lsp_open(&fx.state, doc(&ws.id, "a.ts", "x")).unwrap();
    assert_eq!(starts(), 2);
}

fn start(workspace_id: &str, path: &str) -> DebugStartArgs {
    DebugStartArgs {
        workspace_id: workspace_id.to_owned(),
        path: path.to_owned(),
        breakpoints: vec![FileBreakpoints {
            path: "app.js".into(),
            lines: vec![2],
        }],
    }
}

fn thread(workspace_id: &str, step: Option<StepDto>) -> DebugThreadArgs {
    DebugThreadArgs {
        workspace_id: workspace_id.to_owned(),
        thread_id: 7,
        step,
    }
}

#[test]
fn a_program_runs_under_the_debugger_until_stopped_or_the_workspace_changes() {
    let fx = fixture();
    let ws = handlers::open_picked(&fx.state, &folder("alpha")).unwrap();
    // Nothing running yet.
    assert_eq!(
        code(handlers::debug_resume(&fx.state, thread(&ws.id, None))),
        IpcErrorCode::InvalidArgument
    );
    // Not a file it can run.
    assert_eq!(
        code(handlers::debug_start(&fx.state, start(&ws.id, "notes.md"))),
        IpcErrorCode::InvalidArgument
    );

    handlers::debug_start(&fx.state, start(&ws.id, "app.js")).unwrap();
    let stopped = fx.emitted.lock().unwrap().iter().any(|event| {
        matches!(event, BackendEventDto::Debug { workspace_id, .. } if *workspace_id == ws.id)
    });
    assert!(stopped, "the debugger's events reach the page, tagged");

    handlers::debug_resume(&fx.state, thread(&ws.id, Some(StepDto::Over))).unwrap();
    handlers::debug_resume(&fx.state, thread(&ws.id, None)).unwrap();
    assert_eq!(
        handlers::debug_evaluate(
            &fx.state,
            crate::dto::DebugEvaluateArgs {
                workspace_id: ws.id.clone(),
                expression: "1 + 1".into(),
                frame_id: None,
            },
        )
        .unwrap(),
        "= 1 + 1"
    );
    // Starting again ends the one running.
    handlers::debug_start(&fx.state, start(&ws.id, "app.js")).unwrap();
    // Another workspace ends it too.
    handlers::open_picked(&fx.state, &folder("beta")).unwrap();
    assert_eq!(
        *fx.debuggers.log.lock().unwrap(),
        [
            r#"launch app.js in alpha with [(RelPath("app.js"), [2])]"#,
            "step 7 Over",
            "resume 7",
            "end",
            r#"launch app.js in alpha with [(RelPath("app.js"), [2])]"#,
            "end",
        ]
    );
}

fn agent_config(enabled: bool, allow_edits: bool, port: u16) -> AgentConfigArgs {
    AgentConfigArgs {
        enabled,
        allow_edits,
        port,
    }
}

#[test]
fn the_agent_server_starts_stops_and_restarts_as_configured() {
    let fx = fixture();
    // Off until turned on.
    fx.state.start_agents();
    let status = handlers::agent_status(&fx.state).unwrap();
    assert!(!status.enabled && status.url.is_none() && status.error.is_none());
    assert_eq!(status.token, "t1");
    assert!(fx.agents.log.lock().unwrap().is_empty());

    let status = handlers::agent_configure(&fx.state, agent_config(true, false, 7341)).unwrap();
    assert_eq!(status.url.as_deref(), Some("http://127.0.0.1:7341/mcp"));
    // Edits allowed: the old server goes before the new one starts.
    handlers::agent_configure(&fx.state, agent_config(true, true, 7341)).unwrap();
    // A new token restarts it with that token.
    let status = handlers::agent_new_token(&fx.state).unwrap();
    assert_eq!(status.token, "t1+");
    handlers::agent_configure(&fx.state, agent_config(false, true, 7341)).unwrap();
    assert_eq!(
        *fx.agents.log.lock().unwrap(),
        [
            "serve 7341 edits=false token=t1",
            "stop 7341",
            "serve 7341 edits=true token=t1",
            "stop 7341",
            "serve 7341 edits=true token=t1+",
            "stop 7341",
        ]
    );
    let status = handlers::agent_status(&fx.state).unwrap();
    assert!(!status.enabled && status.url.is_none());
}

#[test]
fn the_agent_server_says_why_it_isnt_running() {
    let fx = fixture();
    let status = handlers::agent_configure(&fx.state, agent_config(true, false, 1999)).unwrap();
    assert!(status.enabled);
    assert!(status.url.is_none());
    assert_eq!(status.error.as_deref(), Some("port 1999 is in use"));
    let refused = handlers::agent_configure(&fx.state, agent_config(true, false, 80));
    assert_eq!(code(refused), IpcErrorCode::InvalidSettings);
    // Started again at launch, from what was saved.
    *fx.agents.config.lock().unwrap() = AgentConfig {
        enabled: true,
        allow_edits: false,
        port: 7400,
    };
    fx.state.start_agents();
    assert!(handlers::agent_status(&fx.state).unwrap().error.is_none());
}

#[test]
fn the_agent_server_answers_about_the_open_workspace_and_reports_calls() {
    let fx = fixture();
    handlers::agent_configure(&fx.state, agent_config(true, false, 7341)).unwrap();
    let (workbench, sink) = fx.agents.workbench.lock().unwrap().clone().unwrap();
    assert!(workbench.root().is_none());
    handlers::open_picked(&fx.state, &folder("alpha")).unwrap();
    assert_eq!(
        workbench
            .root()
            .map(|root| root.name().to_owned())
            .as_deref(),
        Some("alpha")
    );
    handlers::open_picked(&fx.state, &folder("beta")).unwrap();
    assert_eq!(
        workbench
            .root()
            .map(|root| root.name().to_owned())
            .as_deref(),
        Some("beta")
    );
    assert_eq!(workbench.read_limits(), Settings::default().read_limits());

    sink(AgentCall {
        tool: "read_file".to_owned(),
        target: Some("a.ts".to_owned()),
        error: None,
    });
    let emitted = fx.emitted.lock().unwrap();
    let json = serde_json::to_value(emitted.last().unwrap()).unwrap();
    assert_eq!(
        json,
        json!({ "kind": "agentCall", "call": { "tool": "read_file", "target": "a.ts", "error": null } })
    );
}
