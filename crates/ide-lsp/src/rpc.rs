//! JSON-RPC 2.0 over a child process's stdio, framed as LSP frames it
//! (`Content-Length: n\r\n\r\n` and n bytes of JSON).

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

/// The largest message accepted from a server.
const MAX_MESSAGE: usize = 64 << 20;

/// Why a call didn't get a result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RpcError {
    /// The server ended, or never started.
    Closed,
    TimedOut,
    /// The server answered with an error.
    Server {
        code: i64,
        message: String,
    },
    Io(String),
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => write!(f, "the language server isn't running"),
            Self::TimedOut => write!(f, "the language server didn't answer in time"),
            Self::Server { message, .. } => write!(f, "{message}"),
            Self::Io(message) => write!(f, "{message}"),
        }
    }
}

/// Writes one framed message.
pub fn write_message(out: &mut impl Write, message: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(message)?;
    write!(out, "Content-Length: {}\r\n\r\n", body.len())?;
    out.write_all(&body)?;
    out.flush()
}

/// Reads one framed message; `None` at the end of the stream.
pub fn read_message(input: &mut impl BufRead) -> io::Result<Option<Value>> {
    let mut length = None;
    let mut line = String::new();
    loop {
        line.clear();
        if input.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let header = line.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse::<usize>().ok();
        }
    }
    let length = length.ok_or_else(|| io::Error::other("a message without Content-Length"))?;
    if length > MAX_MESSAGE {
        return Err(io::Error::other(format!("a message of {length} bytes")));
    }
    let mut body = vec![0; length];
    input.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(io::Error::other)
}

type Pending = Arc<Mutex<HashMap<i64, mpsc::Sender<Result<Value, RpcError>>>>>;

/// Called with each notification the server sends (method, params).
pub type OnNotification = Box<dyn Fn(&str, Value) + Send>;

/// A server process, and calls to it.
pub struct RpcClient {
    stdin: Arc<Mutex<ChildStdin>>,
    pending: Pending,
    next_id: AtomicI64,
    alive: Arc<AtomicBool>,
    child: Mutex<Child>,
}

impl std::fmt::Debug for RpcClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RpcClient")
            .field("alive", &self.is_alive())
            .finish_non_exhaustive()
    }
}

impl RpcClient {
    /// Starts `program` with `args` in `cwd` (never through a shell), and
    /// reads what it sends. `on_exit` runs once, when it stops talking.
    pub fn spawn(
        program: &Path,
        args: &[String],
        cwd: &Path,
        on_notification: OnNotification,
        on_exit: Box<dyn FnOnce() + Send>,
    ) -> io::Result<Self> {
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: the app has no console, so the server would open one.
            command.creation_flags(0x0800_0000);
        }
        let mut child = command.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("no stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("no stdout"))?;
        // Its log on stderr is read and dropped, so the pipe never fills.
        if let Some(mut stderr) = child.stderr.take() {
            thread::spawn(move || io::copy(&mut stderr, &mut io::sink()));
        }

        let stdin = Arc::new(Mutex::new(stdin));
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let alive = Arc::new(AtomicBool::new(true));
        let (reply_to, read_pending, read_alive) =
            (Arc::clone(&stdin), Arc::clone(&pending), Arc::clone(&alive));
        thread::Builder::new()
            .name("ide-lsp read".to_owned())
            .spawn(move || {
                let mut input = BufReader::new(stdout);
                while let Ok(Some(message)) = read_message(&mut input) {
                    dispatch(message, &read_pending, &reply_to, &on_notification);
                }
                read_alive.store(false, Ordering::Release);
                // Whoever waits for an answer won't get one.
                for (_, waiting) in read_pending
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .drain()
                {
                    let _ = waiting.send(Err(RpcError::Closed));
                }
                on_exit();
            })?;

        Ok(Self {
            stdin,
            pending,
            next_id: AtomicI64::new(1),
            alive,
            child: Mutex::new(child),
        })
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }

    fn send(&self, message: &Value) -> Result<(), RpcError> {
        if !self.is_alive() {
            return Err(RpcError::Closed);
        }
        let mut stdin = self.stdin.lock().unwrap_or_else(PoisonError::into_inner);
        write_message(&mut *stdin, message).map_err(|_| RpcError::Closed)
    }

    /// A request, and its result (or the server's error), within `limit`.
    pub fn request(&self, method: &str, params: Value, limit: Duration) -> Result<Value, RpcError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (answer, answered) = mpsc::channel();
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, answer);
        let sent =
            self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        if let Err(error) = sent {
            self.pending
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&id);
            return Err(error);
        }
        match answered.recv_timeout(limit) {
            Ok(result) => result,
            Err(_) => {
                self.pending
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(&id);
                // Tell the server it needn't finish.
                let _ = self.notify("$/cancelRequest", json!({ "id": id }));
                Err(RpcError::TimedOut)
            }
        }
    }

    /// A notification: no answer.
    pub fn notify(&self, method: &str, params: Value) -> Result<(), RpcError> {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }))
    }
}

impl Drop for RpcClient {
    fn drop(&mut self) {
        // Asked to leave (`shutdown`, `exit`) by its owner; this makes sure.
        let mut child = self.child.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// A message from the server: an answer, a request of its own, or a notification.
fn dispatch(
    message: Value,
    pending: &Pending,
    reply_to: &Mutex<ChildStdin>,
    on_notification: &OnNotification,
) {
    let id = message.get("id").cloned();
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .map(str::to_owned);
    match (id, method) {
        // An answer to one of ours.
        (Some(id), None) => {
            let Some(id) = id.as_i64() else { return };
            let Some(waiting) = pending
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&id)
            else {
                return;
            };
            let result = match message.get("error") {
                Some(error) => Err(RpcError::Server {
                    code: error.get("code").and_then(Value::as_i64).unwrap_or(0),
                    message: error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("the language server failed")
                        .to_owned(),
                }),
                None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
            };
            let _ = waiting.send(result);
        }
        // The server asks something: answered at once, plainly.
        (Some(id), Some(method)) => {
            let result = answer_server_request(&method, message.get("params"));
            let mut stdin = reply_to.lock().unwrap_or_else(PoisonError::into_inner);
            let _ = write_message(
                &mut *stdin,
                &json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            );
        }
        (None, Some(method)) => {
            on_notification(
                &method,
                message.get("params").cloned().unwrap_or(Value::Null),
            );
        }
        (None, None) => {}
    }
}

/// What the client says to a server's own requests. It has no settings to
/// give and nothing to register, so: the defaults, and "fine".
pub fn answer_server_request(method: &str, params: Option<&Value>) -> Value {
    match method {
        // One value per item asked for; null is "use your default".
        "workspace/configuration" => {
            let items = params
                .and_then(|params| params.get("items"))
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            Value::Array(vec![Value::Null; items])
        }
        "workspace/workspaceFolders" => Value::Null,
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip_through_frames() {
        let mut wire = Vec::new();
        write_message(
            &mut wire,
            &json!({ "jsonrpc": "2.0", "id": 1, "result": "é" }),
        )
        .unwrap();
        write_message(&mut wire, &json!({ "jsonrpc": "2.0", "method": "x" })).unwrap();
        let mut input = io::Cursor::new(wire);
        assert_eq!(
            read_message(&mut input).unwrap(),
            Some(json!({ "jsonrpc": "2.0", "id": 1, "result": "é" }))
        );
        assert_eq!(
            read_message(&mut input).unwrap(),
            Some(json!({ "jsonrpc": "2.0", "method": "x" }))
        );
        assert_eq!(read_message(&mut input).unwrap(), None);
    }

    #[test]
    fn headers_are_read_in_any_case_and_extra_ones_ignored() {
        let body = br#"{"a":1}"#;
        let wire = [
            format!(
                "content-length: {}\r\nContent-Type: application/vscode-jsonrpc\r\n\r\n",
                body.len()
            )
            .into_bytes(),
            body.to_vec(),
        ]
        .concat();
        assert_eq!(
            read_message(&mut io::Cursor::new(wire)).unwrap(),
            Some(json!({ "a": 1 }))
        );
    }

    #[test]
    fn a_frame_without_a_length_or_too_large_is_an_error() {
        assert!(read_message(&mut io::Cursor::new(b"X: 1\r\n\r\n{}".to_vec())).is_err());
        let huge = format!("Content-Length: {}\r\n\r\n", MAX_MESSAGE + 1);
        assert!(read_message(&mut io::Cursor::new(huge.into_bytes())).is_err());
    }

    #[test]
    fn server_requests_get_defaults() {
        assert_eq!(
            answer_server_request(
                "workspace/configuration",
                Some(&json!({ "items": [{}, {}] }))
            ),
            json!([null, null])
        );
        assert_eq!(
            answer_server_request("client/registerCapability", None),
            Value::Null
        );
    }
}
