//! Debugging JavaScript and TypeScript with Node, through vscode-js-debug
//! (its DAP server, which ships with the app) and the person's own `node`.
//!
//! js-debug works in two sessions: the first launches the program, then asks
//! (`startDebugging`) for a second connection, where the program's threads,
//! breakpoints and stack are. Both are handled here, as one session.

mod dap;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;
use std::time::Instant;

use ide_core::debug::{
    Breakpoint, DebugAdapters, DebugEvent, DebugSession, DebugSink, FrameSource, Scope, StackFrame,
    Step, Variable,
};
use ide_core::{IdeError, RelPath, WorkspaceRoot};
use serde_json::{Value, json};

pub use crate::dap::{DapConnection, DapError, read_message, write_message};

/// How long the debugger may take to start, and to answer.
const START_LIMIT: Duration = Duration::from_secs(30);
const ASK_LIMIT: Duration = Duration::from_secs(10);

/// [`DebugAdapters`] for Node, on js-debug's DAP server.
#[derive(Debug, Clone)]
pub struct NodeDebugger {
    /// js-debug's `dapDebugServer.js`.
    server: PathBuf,
}

impl NodeDebugger {
    pub fn new(server: PathBuf) -> Self {
        Self { server }
    }
}

fn failed(what: &str, error: impl std::fmt::Display) -> IdeError {
    IdeError::Io(format!("{what}: {error}"))
}

/// The person's `node`: on the PATH, or where installers put it. An app
/// started from the Finder or the Start menu may not have the shell's PATH,
/// and one already running doesn't see a PATH changed since, so the usual
/// places are looked in too: an install made a moment ago is found.
pub fn find_node() -> Option<PathBuf> {
    let name = if cfg!(windows) { "node.exe" } else { "node" };
    let on_path = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .unwrap_or_default();
    on_path
        .into_iter()
        .chain(node_folders())
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// Where Node's installers and version managers put `node`.
fn node_folders() -> Vec<PathBuf> {
    let env = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let mut folders = Vec::new();
    if cfg!(windows) {
        // The installer (and winget's), for everyone or for one person; Volta.
        for (var, rest) in [
            ("ProgramFiles", &["nodejs"][..]),
            ("LOCALAPPDATA", &["Programs", "nodejs"][..]),
            ("LOCALAPPDATA", &["Volta", "bin"][..]),
        ] {
            if let Some(base) = env(var) {
                folders.push(rest.iter().fold(base, |dir, part| dir.join(part)));
            }
        }
    } else {
        // Homebrew (Apple silicon, Intel), MacPorts, the system's, Snap.
        for dir in [
            "/opt/homebrew/bin",
            "/usr/local/bin",
            "/opt/local/bin",
            "/usr/bin",
            "/snap/bin",
        ] {
            folders.push(PathBuf::from(dir));
        }
        if let Some(home) = env("HOME") {
            folders.push(home.join(".volta/bin"));
            // nvm: the newest version installed.
            let mut versions: Vec<PathBuf> = std::fs::read_dir(home.join(".nvm/versions/node"))
                .map(|entries| {
                    entries
                        .flatten()
                        .map(|entry| entry.path().join("bin"))
                        .collect()
                })
                .unwrap_or_default();
            versions.sort();
            folders.extend(versions.into_iter().rev());
        }
    }
    folders
}

/// A path the debugger names, as a workspace path if it's inside `root`
/// (as people write it). Separators and a Windows drive letter's case don't matter.
pub fn workspace_path(root: &str, path: &str) -> Option<RelPath> {
    let normalize = |text: &str| {
        let mut forward = text.replace('\\', "/");
        if forward.as_bytes().get(1) == Some(&b':') {
            let drive = forward[..1].to_ascii_lowercase();
            forward.replace_range(..1, &drive);
        }
        forward.trim_end_matches('/').to_owned()
    };
    let (root, path) = (normalize(root), normalize(path));
    RelPath::parse(path.strip_prefix(&root)?.strip_prefix('/')?).ok()
}

fn native(root: &str, path: &RelPath) -> String {
    let separator = if cfg!(windows) { "\\" } else { "/" };
    format!(
        "{}{separator}{}",
        root.trim_end_matches(['/', '\\']),
        path.as_str().replace('/', separator)
    )
}

impl DebugAdapters for NodeDebugger {
    fn handles(&self, path: &RelPath) -> bool {
        path.file_name()
            .and_then(|name| name.rsplit_once('.'))
            .is_some_and(|(_, extension)| {
                matches!(
                    extension.to_ascii_lowercase().as_str(),
                    "js" | "mjs" | "cjs" | "ts" | "mts" | "cts"
                )
            })
    }

    fn launch(
        &self,
        root: &WorkspaceRoot,
        program: &RelPath,
        breakpoints: &[(RelPath, Vec<u32>)],
        sink: DebugSink,
    ) -> Result<Box<dyn DebugSession>, IdeError> {
        if !self.server.is_file() {
            return Err(IdeError::Io(format!(
                "the debugger isn't installed with this app ({})",
                self.server.display()
            )));
        }
        let node = find_node().ok_or_else(|| {
            IdeError::Io(
                "Node.js isn't installed, or isn't on the PATH: the debugger runs on it".to_owned(),
            )
        })?;
        let folder = root.display_path();
        let (server, port) = start_server(&node, &self.server, Path::new(&folder))?;

        let shared = Arc::new(Shared {
            folder: folder.clone(),
            port,
            sink: Arc::clone(&sink),
            breakpoints: Mutex::new(breakpoints.iter().cloned().collect()),
            child: Mutex::new(None),
        });

        // The first session: it launches the program, and sends its output.
        let (parent_ready, parent_initialized) = mpsc::channel::<()>();
        let events_sink = Arc::clone(&sink);
        let parent_ready = Mutex::new(parent_ready);
        let requests = Arc::clone(&shared);
        let stream = TcpStream::connect(("127.0.0.1", port))
            .map_err(|error| failed("couldn't reach the debugger", error))?;
        let parent = DapConnection::new(
            stream,
            Box::new(move |event, body| match event {
                "initialized" => {
                    let _ = parent_ready
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .send(());
                }
                "output" => output(&events_sink, &body),
                "terminated" => events_sink(DebugEvent::Terminated),
                _ => {}
            }),
            Box::new(move |command, arguments| {
                if command == "startDebugging" {
                    let shared = Arc::clone(&requests);
                    // Answered now; the second session is made on its own thread.
                    thread::spawn(move || shared.start_child(&arguments));
                    Ok(Value::Null)
                } else {
                    Err(format!("{command} isn't supported"))
                }
            }),
            Box::new(|| {}),
        )
        .map_err(|error| failed("couldn't talk to the debugger", error))?;

        parent
            .request("initialize", initialize_arguments(true), START_LIMIT)
            .map_err(|error| failed("the debugger didn't start", error))?;
        parent.fire(
            "launch",
            json!({
                "type": "pwa-node",
                "request": "launch",
                "name": program.file_name().unwrap_or("program"),
                "program": native(&folder, program),
                "cwd": folder,
                "console": "internalConsole",
                "outputCapture": "std",
                // Stepping goes through the program, not Node's own code.
                "skipFiles": ["<node_internals>/**"],
            }),
        );
        parent_initialized
            .recv_timeout(START_LIMIT)
            .map_err(|_| IdeError::Io("the debugger didn't get ready".to_owned()))?;
        parent
            .request("configurationDone", json!({}), START_LIMIT)
            .map_err(|error| failed("the debugger didn't start the program", error))?;

        Ok(Box::new(Session {
            parent,
            shared,
            server: Mutex::new(server),
        }))
    }
}

/// Runs js-debug's server, and leaves when its stdin closes: when the session
/// ends, or the app does. (`node` may be a shim, such as pnpm's or Volta's,
/// that starts the real one as its own child; killing the shim would leave
/// the server running.) `process.argv` is then what the server expects.
const SERVER_WRAPPER: &str = "process.stdin.on('end',()=>process.exit(0));process.stdin.on('error',()=>process.exit(0));process.stdin.resume();require(process.argv[1]);";

/// `dapDebugServer.js 0 127.0.0.1`: it says which port it took.
fn start_server(node: &Path, script: &Path, cwd: &Path) -> Result<(Child, u16), IdeError> {
    let mut command = Command::new(node);
    command
        .args(["-e", SERVER_WRAPPER])
        .arg(script)
        .args(["0", "127.0.0.1"])
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: the app has no console, so Node would open one.
        command.creation_flags(0x0800_0000);
    }
    let mut child = command
        .spawn()
        .map_err(|error| failed("couldn't start the debugger", error))?;
    let stdout = child.stdout.take();
    if let Some(mut stderr) = child.stderr.take() {
        thread::spawn(move || std::io::copy(&mut stderr, &mut std::io::sink()));
    }
    let (found, port) = mpsc::channel();
    thread::spawn(move || {
        let Some(stdout) = stdout else { return };
        let mut lines = BufReader::new(stdout);
        let mut line = String::new();
        while lines.read_line(&mut line).is_ok_and(|read| read > 0) {
            // "Debug server listening at 127.0.0.1:53470"
            if let Some(port) = line
                .trim()
                .rsplit(':')
                .next()
                .and_then(|port| port.parse::<u16>().ok())
                && line.contains("listening")
            {
                let _ = found.send(port);
                break;
            }
            line.clear();
        }
        // Read the rest, so the pipe never fills.
        let _ = std::io::copy(&mut lines.into_inner().take(u64::MAX), &mut std::io::sink());
    });
    match port.recv_timeout(START_LIMIT) {
        Ok(port) => Ok((child, port)),
        Err(_) => {
            let _ = child.kill();
            Err(IdeError::Io("the debugger didn't start in time".to_owned()))
        }
    }
}

fn initialize_arguments(start_debugging: bool) -> Value {
    json!({
        "adapterID": "pwa-node",
        "clientID": "webide",
        "clientName": "webide",
        "linesStartAt1": true,
        "columnsStartAt1": true,
        "pathFormat": "path",
        "supportsVariableType": true,
        "supportsStartDebuggingRequest": start_debugging,
    })
}

fn output(sink: &DebugSink, body: &Value) {
    let text = body.get("output").and_then(Value::as_str).unwrap_or("");
    let category = body
        .get("category")
        .and_then(Value::as_str)
        .unwrap_or("console");
    // js-debug's own notes ("js-debug/launch") aren't the program's.
    if text.is_empty() || category == "telemetry" {
        return;
    }
    sink(DebugEvent::Output {
        category: category.to_owned(),
        text: text.to_owned(),
    });
}

/// What both sessions share.
struct Shared {
    folder: String,
    port: u16,
    sink: DebugSink,
    /// Every file's breakpoint lines, as asked for.
    breakpoints: Mutex<HashMap<RelPath, Vec<u32>>>,
    /// The second session, where the program is, once it's ready.
    child: Mutex<Option<Arc<DapConnection>>>,
}

impl Shared {
    fn start_child(self: &Arc<Self>, arguments: &Value) {
        let configuration = arguments
            .get("configuration")
            .cloned()
            .unwrap_or(Value::Null);
        let Ok(stream) = TcpStream::connect(("127.0.0.1", self.port)) else {
            return;
        };
        let (ready, initialized) = mpsc::channel::<()>();
        let ready = Mutex::new(ready);
        let sink = Arc::clone(&self.sink);
        let Ok(child) = DapConnection::new(
            stream,
            Box::new(move |event, body| match event {
                "initialized" => {
                    let _ = ready
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .send(());
                }
                "stopped" => sink(DebugEvent::Stopped {
                    thread_id: body.get("threadId").and_then(Value::as_i64).unwrap_or(0),
                    reason: body
                        .get("reason")
                        .and_then(Value::as_str)
                        .unwrap_or("pause")
                        .to_owned(),
                    description: body
                        .get("description")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                }),
                "continued" => sink(DebugEvent::Continued),
                "output" => output(&sink, &body),
                _ => {}
            }),
            Box::new(|command, _| Err(format!("{command} isn't supported"))),
            Box::new(|| {}),
        ) else {
            return;
        };
        let child = Arc::new(child);
        if child
            .request("initialize", initialize_arguments(false), START_LIMIT)
            .is_err()
        {
            return;
        }
        // Its answer comes once it's configured: not waited for.
        child.fire("launch", configuration);
        if initialized.recv_timeout(START_LIMIT).is_err() {
            return;
        }
        let files: Vec<(RelPath, Vec<u32>)> = self
            .breakpoints
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|(path, lines)| (path.clone(), lines.clone()))
            .collect();
        for (path, lines) in files {
            let _ = set_breakpoints(&child, &self.folder, &path, &lines);
        }
        let _ = child.request("configurationDone", json!({}), START_LIMIT);
        *self.child.lock().unwrap_or_else(PoisonError::into_inner) = Some(child);
    }

    fn child(&self) -> Result<Arc<DapConnection>, IdeError> {
        self.child
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or_else(|| {
                IdeError::Io("the program isn't running under the debugger yet".to_owned())
            })
    }
}

fn set_breakpoints(
    child: &DapConnection,
    folder: &str,
    path: &RelPath,
    lines: &[u32],
) -> Result<Vec<Breakpoint>, IdeError> {
    let body = child
        .request(
            "setBreakpoints",
            json!({
                "source": { "path": native(folder, path) },
                "breakpoints": lines.iter().map(|line| json!({ "line": line })).collect::<Vec<_>>(),
            }),
            ASK_LIMIT,
        )
        .map_err(|error| failed("couldn't set the breakpoints", error))?;
    let placed = body
        .get("breakpoints")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(lines
        .iter()
        .enumerate()
        .map(|(index, &line)| {
            let found = placed.get(index);
            let actual = found
                .and_then(|found| found.get("line"))
                .and_then(Value::as_u64)
                .and_then(|line| u32::try_from(line).ok());
            Breakpoint {
                line,
                verified: found
                    .and_then(|found| found.get("verified"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                actual_line: actual.filter(|&actual| actual != line),
            }
        })
        .collect())
}

struct Session {
    parent: DapConnection,
    shared: Arc<Shared>,
    server: Mutex<Child>,
}

impl Session {
    fn ask(&self, command: &str, arguments: Value) -> Result<Value, IdeError> {
        self.shared
            .child()?
            .request(command, arguments, ASK_LIMIT)
            .map_err(|error| failed(command, error))
    }
}

impl DebugSession for Session {
    fn set_breakpoints(&self, path: &RelPath, lines: &[u32]) -> Result<Vec<Breakpoint>, IdeError> {
        self.shared
            .breakpoints
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(path.clone(), lines.to_vec());
        match self.shared.child() {
            Ok(child) => set_breakpoints(&child, &self.shared.folder, path, lines),
            // Not started yet: they're set when it is.
            Err(_) => Ok(lines
                .iter()
                .map(|&line| Breakpoint {
                    line,
                    verified: false,
                    actual_line: None,
                })
                .collect()),
        }
    }

    fn resume(&self, thread_id: i64) -> Result<(), IdeError> {
        self.ask("continue", json!({ "threadId": thread_id }))
            .map(drop)
    }

    fn step(&self, thread_id: i64, step: Step) -> Result<(), IdeError> {
        let command = match step {
            Step::Over => "next",
            Step::Into => "stepIn",
            Step::Out => "stepOut",
        };
        self.ask(command, json!({ "threadId": thread_id }))
            .map(drop)
    }

    fn pause(&self, thread_id: i64) -> Result<(), IdeError> {
        self.ask("pause", json!({ "threadId": thread_id }))
            .map(drop)
    }

    fn stack(&self, thread_id: i64) -> Result<Vec<StackFrame>, IdeError> {
        let body = self.ask(
            "stackTrace",
            json!({ "threadId": thread_id, "levels": 100 }),
        )?;
        Ok(body
            .get("stackFrames")
            .and_then(Value::as_array)
            .map(|frames| {
                frames
                    .iter()
                    .map(|frame| {
                        let path = frame
                            .get("source")
                            .and_then(|source| source.get("path"))
                            .and_then(Value::as_str);
                        let name = frame
                            .get("source")
                            .and_then(|source| source.get("name"))
                            .and_then(Value::as_str);
                        StackFrame {
                            id: frame.get("id").and_then(Value::as_i64).unwrap_or(0),
                            name: frame
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or("?")
                                .to_owned(),
                            source: match path {
                                Some(path) => {
                                    Some(match workspace_path(&self.shared.folder, path) {
                                        Some(inside) => FrameSource::Workspace(inside),
                                        None => FrameSource::Outside(path.to_owned()),
                                    })
                                }
                                None => name.map(|name| FrameSource::Outside(name.to_owned())),
                            },
                            line: number(frame.get("line")),
                            column: number(frame.get("column")),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    fn scopes(&self, frame_id: i64) -> Result<Vec<Scope>, IdeError> {
        let body = self.ask("scopes", json!({ "frameId": frame_id }))?;
        Ok(body
            .get("scopes")
            .and_then(Value::as_array)
            .map(|scopes| {
                scopes
                    .iter()
                    .map(|scope| Scope {
                        name: text(scope.get("name")),
                        reference: scope
                            .get("variablesReference")
                            .and_then(Value::as_i64)
                            .unwrap_or(0),
                        expensive: scope
                            .get("expensive")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    fn variables(&self, reference: i64) -> Result<Vec<Variable>, IdeError> {
        let body = self.ask("variables", json!({ "variablesReference": reference }))?;
        Ok(body
            .get("variables")
            .and_then(Value::as_array)
            .map(|variables| {
                variables
                    .iter()
                    .map(|variable| Variable {
                        name: text(variable.get("name")),
                        value: text(variable.get("value")),
                        kind: variable
                            .get("type")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        reference: variable
                            .get("variablesReference")
                            .and_then(Value::as_i64)
                            .unwrap_or(0),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    fn evaluate(&self, expression: &str, frame_id: Option<i64>) -> Result<String, IdeError> {
        let mut arguments = json!({ "expression": expression, "context": "repl" });
        if let Some(frame) = frame_id {
            arguments["frameId"] = json!(frame);
        }
        let body = self.ask("evaluate", arguments)?;
        Ok(text(body.get("result")))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // End the program, then the debugger.
        if let Ok(child) = self.shared.child() {
            let _ = child.request(
                "disconnect",
                json!({ "terminateDebuggee": true }),
                Duration::from_secs(2),
            );
            child.close();
        }
        let _ = self.parent.request(
            "disconnect",
            json!({ "terminateDebuggee": true }),
            Duration::from_secs(2),
        );
        self.parent.close();
        let mut server = self.server.lock().unwrap_or_else(PoisonError::into_inner);
        // Its stdin closing ends it (through any shim); then make sure.
        drop(server.stdin.take());
        let _ = server.wait_timeout_or_kill();
    }
}

fn number(value: Option<&Value>) -> u32 {
    value
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(0)
}

fn text(value: Option<&Value>) -> String {
    value.and_then(Value::as_str).unwrap_or("").to_owned()
}

/// Waiting for a child with a limit, then killing it.
trait WaitOrKill {
    fn wait_timeout_or_kill(&mut self) -> std::io::Result<()>;
}

impl WaitOrKill for Child {
    fn wait_timeout_or_kill(&mut self) -> std::io::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if self.try_wait()?.is_some() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(20));
        }
        self.kill()?;
        self.wait().map(drop)
    }
}
