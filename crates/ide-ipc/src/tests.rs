use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ide_core::fs::{
    DirListing, FileNode, FileStamp, LineEnding, NodeKind, ReadLimits, ReadOutcome,
};
use ide_core::search::line_match;
use ide_core::{
    CancelFlag, FileList, FileMatches, FsChange, FsWatcher, IdeError, RecentWorkspace, RelPath,
    Settings, SettingsPatch, TextQuery, TextSearchOutcome, WatchEvent, WatchGuard, WatchSink,
    WorkspaceFs, WorkspaceRoot, WorkspaceSearch, WorkspaceStore,
};
use serde_json::json;

use crate::dto::{
    AppInfoDto, BackendEventDto, CloseGuardArgs, OpenRecentArgs, ReadFileDto, SearchTextArgs,
    SettingsPatchDto, WorkspaceArgs, WorkspacePathArgs, WriteFileArgs,
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

fn folder(name: &str) -> PathBuf {
    std::env::temp_dir().join(name)
}

struct Fixture {
    state: IpcState,
    fs: Arc<FakeFs>,
    store: Arc<FakeStore>,
    watcher: Arc<FakeWatcher>,
    search: Arc<FakeSearch>,
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
    let services = Services {
        fs: fs.clone(),
        workspace: store.clone(),
        watcher: watcher.clone(),
        search: search.clone(),
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
