use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex, PoisonError};

use ide_core::agent::{AgentCall, AgentConfig, AgentHost, AgentServer, AgentWorkbench};
use ide_core::fs::{
    DirListing, FileNode, FileStamp, LineEnding, NodeKind, ReadLimits, ReadOutcome,
};
use ide_core::git::{
    DiffSide, DiffTexts, GitBranch, GitChange, GitFileStatus, GitHead, GitState, GitStatus,
};
use ide_core::{
    CancelFlag, FileList, FileMatches, GitClient, IdeError, LineMatch, RelPath, TextQuery,
    TextSearchOutcome, WorkspaceFs, WorkspaceRoot, WorkspaceSearch,
};
use serde_json::{Value, json};

use crate::{McpHost, unified_diff, workspace_relative};

// ---- The diff ----

#[test]
fn diffs_by_line_with_context() {
    let original = "a\nb\nc\nd\ne\nf\ng\nh\n";
    let modified = "a\nb\nc\nD\ne\nf\ng\nh\n";
    assert_eq!(
        unified_diff("x.txt", original, modified),
        "--- a/x.txt\n+++ b/x.txt\n@@ -1,7 +1,7 @@\n a\n b\n c\n-d\n+D\n e\n f\n g\n"
    );
    assert_eq!(unified_diff("x.txt", original, original), "");
}

#[test]
fn diffs_additions_removals_and_a_missing_final_newline() {
    assert_eq!(
        unified_diff("n.txt", "", "one\ntwo\n"),
        "--- a/n.txt\n+++ b/n.txt\n@@ -0,0 +1,2 @@\n+one\n+two\n"
    );
    assert_eq!(
        unified_diff("n.txt", "keep\ngone\n", "keep\n"),
        "--- a/n.txt\n+++ b/n.txt\n@@ -1,2 +1,1 @@\n keep\n-gone\n"
    );
    assert_eq!(
        unified_diff("n.txt", "end", "end\n"),
        "--- a/n.txt\n+++ b/n.txt\n@@ -1,1 +1,1 @@\n-end\n\\ No newline at end of file\n+end\n"
    );
}

#[test]
fn separate_changes_get_separate_hunks() {
    let original: String = (1..=20).map(|n| format!("{n}\n")).collect();
    let modified: String = (1..=20)
        .map(|n| match n {
            2 => "two\n".to_owned(),
            19 => "nineteen\n".to_owned(),
            n => format!("{n}\n"),
        })
        .collect();
    let diff = unified_diff("f", &original, &modified);
    let headers: Vec<&str> = diff.lines().filter(|line| line.starts_with("@@")).collect();
    assert_eq!(headers, ["@@ -1,5 +1,5 @@", "@@ -16,5 +16,5 @@"]);
}

// ---- Paths ----

fn root() -> (tempfile::TempDir, WorkspaceRoot) {
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let path = dir
        .path()
        .canonicalize()
        .unwrap_or_else(|error| panic!("{error}"));
    let root = WorkspaceRoot::from_canonical(path).unwrap_or_else(|error| panic!("{error}"));
    (dir, root)
}

#[test]
fn takes_relative_and_absolute_paths_inside_the_workspace() {
    let (_dir, root) = root();
    let base = root.display_path();
    let ok = |input: &str| workspace_relative(&root, input).map(|path| path.as_str().to_owned());
    assert_eq!(ok("src/a.ts"), Ok("src/a.ts".to_owned()));
    assert_eq!(ok("./src\\a.ts"), Ok("src/a.ts".to_owned()));
    assert_eq!(ok("."), Ok(String::new()));
    assert_eq!(ok(&format!("{base}/src/a.ts")), Ok("src/a.ts".to_owned()));
    assert_eq!(ok(&base), Ok(String::new()));
    assert!(ok("../outside.txt").is_err());
    assert!(ok("/etc/passwd").is_err());
    assert!(ok(&format!("{base}-other/x")).is_err());
}

// ---- Configuration and token ----

fn host(dir: &std::path::Path, fs: Arc<FakeFs>) -> McpHost {
    McpHost::new(
        dir.to_path_buf(),
        fs,
        Arc::new(FakeSearch),
        Arc::new(FakeGit),
    )
}

#[test]
fn keeps_its_configuration_and_token() {
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let host = host(dir.path(), Arc::new(FakeFs::default()));
    assert_eq!(host.config().ok(), Some(AgentConfig::default()));
    assert!(!AgentConfig::default().enabled);

    let config = AgentConfig {
        enabled: true,
        allow_edits: true,
        port: 9000,
    };
    assert!(host.save_config(&config).is_ok());
    assert_eq!(host.config().ok(), Some(config));
    assert!(
        host.save_config(&AgentConfig { port: 80, ..config })
            .is_err()
    );

    let token = host.token().unwrap_or_default();
    assert_eq!(token.len(), 64);
    assert_eq!(host.token().ok(), Some(token.clone()));
    let next = host.new_token().unwrap_or_default();
    assert_ne!(next, token);
    assert_eq!(host.token().ok(), Some(next));

    // A damaged file is replaced, not trusted.
    assert!(std::fs::write(dir.path().join("agent-token"), "short").is_ok());
    assert_eq!(host.token().map(|token| token.len()).ok(), Some(64));
    assert!(std::fs::write(dir.path().join("agents.json"), "{ not json").is_ok());
    assert_eq!(host.config().ok(), Some(AgentConfig::default()));
}

#[test]
fn says_when_the_port_is_taken() {
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let host = host(dir.path(), Arc::new(FakeFs::default()));
    let taken =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap_or_else(|error| panic!("{error}"));
    let port = taken
        .local_addr()
        .map(|addr| addr.port())
        .unwrap_or_default();
    let token = host.token().unwrap_or_default();
    let config = AgentConfig {
        enabled: true,
        allow_edits: false,
        port,
    };
    let result = host.serve(&config, &token, Arc::new(Bench(None)), Arc::new(|_| {}));
    match result {
        Err(IdeError::Io(message)) => assert!(message.contains("in use"), "{message}"),
        Err(other) => panic!("{other}"),
        Ok(_) => panic!("served on a port in use"),
    }
}

// ---- The server, over HTTP ----

struct Server {
    _dir: tempfile::TempDir,
    _root_dir: tempfile::TempDir,
    server: Box<dyn AgentServer>,
    token: String,
    fs: Arc<FakeFs>,
    calls: Arc<Mutex<Vec<AgentCall>>>,
}

fn start(allow_edits: bool) -> Server {
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let (root_dir, root) = root();
    let fs = Arc::new(FakeFs::default());
    fs.put("src/main.ts", "one\ntwo\nthree\n");
    let host = host(dir.path(), Arc::clone(&fs));
    let token = host.token().unwrap_or_else(|error| panic!("{error}"));
    let calls = Arc::new(Mutex::new(Vec::new()));
    let sink = {
        let calls = Arc::clone(&calls);
        Arc::new(move |call| {
            calls
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(call)
        })
    };
    let listener =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap_or_else(|error| panic!("{error}"));
    let server = host
        .serve_on(
            listener,
            allow_edits,
            &token,
            Arc::new(Bench(Some(root))),
            sink,
        )
        .unwrap_or_else(|error| panic!("{error}"));
    Server {
        _dir: dir,
        _root_dir: root_dir,
        server,
        token,
        fs,
        calls,
    }
}

/// One HTTP/1.1 request; the status and the body.
fn post(url: &str, headers: &[(&str, &str)], body: &Value) -> (u16, String) {
    let authority = url
        .strip_prefix("http://")
        .and_then(|rest| rest.split('/').next())
        .unwrap_or_default();
    let path = &url["http://".len() + authority.len()..];
    // A `Host` given in `headers` replaces the real one.
    let host = headers
        .iter()
        .find(|(name, _)| *name == "Host")
        .map_or(authority, |(_, value)| *value);
    let body = body.to_string();
    let mut request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (name, value) in headers.iter().filter(|(name, _)| *name != "Host") {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str("\r\n");
    request.push_str(&body);
    let mut stream = TcpStream::connect(authority).unwrap_or_else(|error| panic!("{error}"));
    stream
        .write_all(request.as_bytes())
        .unwrap_or_else(|error| panic!("{error}"));
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .unwrap_or_else(|error| panic!("{error}"));
    let response = String::from_utf8_lossy(&response).into_owned();
    let (head, body) = response.split_once("\r\n\r\n").unwrap_or((&response, ""));
    let status = head
        .split(' ')
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or(0);
    let chunked = head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked");
    (
        status,
        if chunked {
            unchunk(body)
        } else {
            body.to_owned()
        },
    )
}

fn unchunk(mut body: &str) -> String {
    let mut out = String::new();
    while let Some((size, rest)) = body.split_once("\r\n") {
        let size = usize::from_str_radix(size.trim(), 16).unwrap_or(0);
        if size == 0 || rest.len() < size {
            break;
        }
        out.push_str(&rest[..size]);
        body = rest[size..].trim_start_matches("\r\n");
    }
    out
}

impl Server {
    fn url(&self) -> String {
        self.server.url()
    }

    fn rpc(&self, method: &str, params: Value) -> Value {
        let bearer = format!("Bearer {}", self.token);
        let (status, body) = post(
            &self.url(),
            &[
                ("Authorization", &bearer),
                ("MCP-Protocol-Version", "2025-06-18"),
            ],
            &json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }),
        );
        assert_eq!(status, 200, "{method}: {body}");
        serde_json::from_str(&body).unwrap_or_else(|error| panic!("{error}: {body}"))
    }

    fn call(&self, tool: &str, arguments: Value) -> (bool, String) {
        let answer = self.rpc(
            "tools/call",
            json!({ "name": tool, "arguments": arguments }),
        );
        if let Some(message) = answer["error"]["message"].as_str() {
            return (false, message.to_owned());
        }
        let result = &answer["result"];
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        (result["isError"].as_bool() != Some(true), text)
    }

    fn tool_names(&self) -> Vec<String> {
        let answer = self.rpc("tools/list", json!({}));
        let mut names: Vec<String> = answer["result"]["tools"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
            .collect();
        names.sort();
        names
    }
}

#[test]
fn refuses_requests_without_the_token_from_pages_or_elsewhere() {
    let server = start(false);
    let url = server.url();
    let ping = json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" });
    assert_eq!(post(&url, &[], &ping).0, 401);
    assert_eq!(
        post(&url, &[("Authorization", "Bearer nope")], &ping).0,
        401
    );
    let bearer = format!("Bearer {}", server.token);
    let from_page = [
        ("Authorization", bearer.as_str()),
        ("Origin", "https://example.com"),
    ];
    assert_eq!(post(&url, &from_page, &ping).0, 403);
    assert_eq!(
        post(
            &url.replace("/mcp", "/other"),
            &[("Authorization", &bearer)],
            &ping
        )
        .0,
        404
    );
    // A page on another site whose name resolves to 127.0.0.1 (DNS rebinding).
    let rebound = [
        ("Authorization", bearer.as_str()),
        ("Host", "evil.example.com"),
    ];
    assert_eq!(post(&url, &rebound, &ping).0, 403);
}

#[test]
fn introduces_itself_and_lists_read_tools_only() {
    let server = start(false);
    let answer = server.rpc(
        "initialize",
        json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "test", "version": "1" }
        }),
    );
    assert_eq!(answer["result"]["serverInfo"]["name"], "webide");
    assert!(
        answer["result"]["instructions"]
            .as_str()
            .is_some_and(|text| text.contains("read only"))
    );
    assert_eq!(
        server.tool_names(),
        [
            "find_files",
            "git_diff",
            "git_status",
            "list_directory",
            "read_file",
            "search_text",
            "workspace_info"
        ]
    );
}

#[test]
fn reads_the_workspace_and_reports_each_call() {
    let server = start(false);
    assert_eq!(
        server.call("read_file", json!({ "path": "src/main.ts" })),
        (true, "one\ntwo\nthree\n".to_owned())
    );
    assert_eq!(
        server.call(
            "read_file",
            json!({ "path": "src/main.ts", "start_line": 2, "end_line": 2 })
        ),
        (true, "two\n".to_owned())
    );
    let (ok, message) = server.call("read_file", json!({ "path": "../secret" }));
    assert!(!ok, "{message}");
    assert_eq!(
        server.call("list_directory", json!({})),
        (true, "src/\nREADME.md (12 bytes)\n".to_owned())
    );
    // .git is neither listed nor read: its config can hold credentials.
    server
        .fs
        .put(".git/config", "url = https://user:secret@example.com/r.git");
    for (tool, args) in [
        ("read_file", json!({ "path": ".git/config" })),
        ("read_file", json!({ "path": "src/../.git/config" })),
        ("read_file", json!({ "path": ".GIT/config" })),
        ("list_directory", json!({ "path": ".git" })),
        ("git_diff", json!({ "path": ".git/config" })),
    ] {
        let (ok, message) = server.call(tool, args);
        assert!(!ok, "{tool}: {message}");
        assert!(!message.contains("secret"), "{message}");
    }
    assert_eq!(
        server.call("find_files", json!({ "query": "srcmn" })),
        (true, "src/main.ts\n".to_owned())
    );
    assert_eq!(
        server.call("search_text", json!({ "pattern": "two" })),
        (true, "src/main.ts:2:1: two\n".to_owned())
    );
    assert_eq!(
        server.call("git_status", json!({})),
        (
            true,
            "on branch main\n M src/main.ts\n?? new.txt\n".to_owned()
        )
    );
    let (ok, diff) = server.call("git_diff", json!({ "path": "src/main.ts" }));
    assert!(ok);
    assert!(diff.contains("-two\n+TWO\n"), "{diff}");
    // Not offered, and refused if called anyway.
    let (ok, _) = server.call("write_file", json!({ "path": "a.txt", "text": "x" }));
    assert!(!ok);
    assert!(server.fs.get("a.txt").is_none());

    let calls = server.calls.lock().unwrap_or_else(PoisonError::into_inner);
    assert_eq!(calls[0].tool, "read_file");
    assert_eq!(calls[0].target.as_deref(), Some("src/main.ts"));
    assert!(calls[0].error.is_none());
    assert!(calls[2].error.is_some());
}

#[test]
fn writes_files_only_when_edits_are_allowed_and_never_in_git() {
    let server = start(true);
    assert!(server.tool_names().contains(&"write_file".to_owned()));
    assert_eq!(
        server.call(
            "write_file",
            json!({ "path": "src/new.ts", "text": "hello" })
        ),
        (true, "wrote src/new.ts (5 bytes)".to_owned())
    );
    assert_eq!(server.fs.get("src/new.ts").as_deref(), Some("hello"));
    let (ok, message) = server.call(
        "write_file",
        json!({ "path": ".git/hooks/pre-commit", "text": "x" }),
    );
    assert!(!ok);
    assert!(message.contains("git directory"), "{message}");
    assert!(server.fs.get(".git/hooks/pre-commit").is_none());
}

#[test]
fn says_when_no_folder_is_open() {
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let host = host(dir.path(), Arc::new(FakeFs::default()));
    let token = host.token().unwrap_or_default();
    let listener =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap_or_else(|error| panic!("{error}"));
    let server = Server {
        server: host
            .serve_on(
                listener,
                false,
                &token,
                Arc::new(Bench(None)),
                Arc::new(|_| {}),
            )
            .unwrap_or_else(|error| panic!("{error}")),
        _dir: dir,
        _root_dir: tempfile::tempdir().unwrap_or_else(|error| panic!("{error}")),
        token,
        fs: Arc::new(FakeFs::default()),
        calls: Arc::default(),
    };
    let (ok, message) = server.call("workspace_info", json!({}));
    assert!(!ok);
    assert_eq!(message, "no folder is open in the IDE");
}

// ---- Fakes ----

struct Bench(Option<WorkspaceRoot>);

impl AgentWorkbench for Bench {
    fn root(&self) -> Option<WorkspaceRoot> {
        self.0.clone()
    }
    fn read_limits(&self) -> ReadLimits {
        ReadLimits {
            read_only_above: 1 << 20,
            refuse_above: 1 << 20,
        }
    }
}

#[derive(Default)]
struct FakeFs {
    files: Mutex<BTreeMap<String, String>>,
}

impl FakeFs {
    fn put(&self, path: &str, text: &str) {
        self.files
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(path.to_owned(), text.to_owned());
    }
    fn get(&self, path: &str) -> Option<String> {
        self.files
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(path)
            .cloned()
    }
}

fn stamp(size: usize) -> FileStamp {
    FileStamp {
        size: size as u64,
        modified_ms: None,
    }
}

fn rel(path: &str) -> RelPath {
    RelPath::parse(path).unwrap_or_else(|error| panic!("{error}"))
}

impl WorkspaceFs for FakeFs {
    fn list_dir(&self, _root: &WorkspaceRoot, dir: &RelPath) -> Result<DirListing, IdeError> {
        if !dir.is_root() {
            return Err(IdeError::NotFound);
        }
        Ok(DirListing {
            path: RelPath::root(),
            entries: vec![
                FileNode {
                    name: ".git".to_owned(),
                    path: rel(".git"),
                    kind: NodeKind::Directory,
                    is_symlink: false,
                    size: None,
                },
                FileNode {
                    name: "src".to_owned(),
                    path: rel("src"),
                    kind: NodeKind::Directory,
                    is_symlink: false,
                    size: None,
                },
                FileNode {
                    name: "README.md".to_owned(),
                    path: rel("README.md"),
                    kind: NodeKind::File,
                    is_symlink: false,
                    size: Some(12),
                },
            ],
            truncated: false,
            skipped: 0,
        })
    }

    fn read_file(
        &self,
        _root: &WorkspaceRoot,
        file: &RelPath,
        _limits: ReadLimits,
    ) -> Result<ReadOutcome, IdeError> {
        let text = self.get(file.as_str()).ok_or(IdeError::NotFound)?;
        Ok(ReadOutcome::Text {
            stamp: stamp(text.len()),
            text,
            line_ending: LineEnding::Lf,
            read_only: None,
        })
    }

    fn write_file(
        &self,
        _root: &WorkspaceRoot,
        file: &RelPath,
        text: &str,
    ) -> Result<FileStamp, IdeError> {
        self.put(file.as_str(), text);
        Ok(stamp(text.len()))
    }
}

struct FakeSearch;

impl WorkspaceSearch for FakeSearch {
    fn search_text(
        &self,
        _root: &WorkspaceRoot,
        query: &TextQuery,
        _cancel: &CancelFlag,
    ) -> Result<TextSearchOutcome, IdeError> {
        Ok(TextSearchOutcome {
            files: vec![FileMatches {
                path: rel("src/main.ts"),
                lines: vec![LineMatch {
                    line: 2,
                    column: 1,
                    preview: query.pattern.clone(),
                    spans: Vec::new(),
                }],
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
            files: vec![rel("README.md"), rel("src/main.ts"), rel("src/other.ts")],
            truncated: false,
        })
    }
}

struct FakeGit;

impl GitClient for FakeGit {
    fn state(&self, _root: &WorkspaceRoot) -> Result<GitState, IdeError> {
        Ok(GitState::Repository(GitStatus {
            head: GitHead {
                branch: Some("main".to_owned()),
                commit: Some("abc1234".to_owned()),
                upstream: None,
                ahead: 0,
                behind: 0,
            },
            files: vec![
                GitFileStatus {
                    path: rel("src/main.ts"),
                    from: None,
                    staged: None,
                    unstaged: Some(GitChange::Modified),
                },
                GitFileStatus {
                    path: rel("new.txt"),
                    from: None,
                    staged: Some(GitChange::Untracked),
                    unstaged: Some(GitChange::Untracked),
                },
            ],
            truncated: false,
        }))
    }

    fn diff_texts(
        &self,
        _root: &WorkspaceRoot,
        _path: &RelPath,
        _side: DiffSide,
        _limit: u64,
    ) -> Result<DiffTexts, IdeError> {
        Ok(DiffTexts::Text {
            original: "one\ntwo\nthree\n".to_owned(),
            modified: "one\nTWO\nthree\n".to_owned(),
        })
    }

    fn stage(&self, _root: &WorkspaceRoot, _paths: &[RelPath]) -> Result<(), IdeError> {
        Err(IdeError::Git("not here".to_owned()))
    }
    fn unstage(&self, _root: &WorkspaceRoot, _paths: &[RelPath]) -> Result<(), IdeError> {
        Err(IdeError::Git("not here".to_owned()))
    }
    fn discard(&self, _root: &WorkspaceRoot, _paths: &[RelPath]) -> Result<(), IdeError> {
        Err(IdeError::Git("not here".to_owned()))
    }
    fn commit(&self, _root: &WorkspaceRoot, _message: &str) -> Result<(), IdeError> {
        Err(IdeError::Git("not here".to_owned()))
    }
    fn branches(&self, _root: &WorkspaceRoot) -> Result<Vec<GitBranch>, IdeError> {
        Ok(Vec::new())
    }
    fn switch_branch(
        &self,
        _root: &WorkspaceRoot,
        _name: &str,
        _create: bool,
    ) -> Result<(), IdeError> {
        Err(IdeError::Git("not here".to_owned()))
    }
    fn init(&self, _root: &WorkspaceRoot) -> Result<(), IdeError> {
        Err(IdeError::Git("not here".to_owned()))
    }
}

// ---- Git directories on disk ----

#[test]
fn refuses_git_directories_however_they_are_named() {
    use crate::tools::{looks_like_git_dir, refuse_git};
    let (dir, root) = root();
    let base = dir.path();
    let make =
        |path: &str| std::fs::create_dir_all(base.join(path)).unwrap_or_else(|e| panic!("{e}"));
    let file = |path: &str| std::fs::write(base.join(path), "x").unwrap_or_else(|e| panic!("{e}"));
    make(".git/objects");
    make(".git/refs");
    file(".git/HEAD");
    file(".git/config");
    make("src");
    // A git directory by another name: a bare repository, or a `gitdir:` target.
    make("data/objects");
    make("data/refs");
    file("data/HEAD");
    file("data/config");

    let check = |path: &str| refuse_git(&root, &rel(path), "read");
    assert!(check(".git/config").is_err());
    assert!(check(".GIT/config").is_err());
    assert!(check("data/config").is_err());
    assert!(check("data").is_err());
    assert!(check("data/hooks/pre-commit").is_err(), "a new file in it");
    assert!(check("src/new.ts").is_ok());
    assert!(check("src").is_ok());
    assert!(check("missing/deep/file.ts").is_ok(), "left to the ports");
    assert!(!looks_like_git_dir(root.path()));
    assert!(looks_like_git_dir(&base.join("data")));

    // A symlink to .git, as a repository can ship one (when this OS lets
    // tests make one: Windows needs developer mode).
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(base.join(".git"), base.join("link")).is_ok();
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_dir(base.join(".git"), base.join("link")).is_ok();
    if linked {
        assert!(check("link/config").is_err());
        assert!(check("link/hooks/pre-commit").is_err());
    }
    // Windows' short name for .git, where the volume makes them.
    if std::fs::metadata(base.join("GIT~1")).is_ok() {
        assert!(check("GIT~1/config").is_err());
    }
}
