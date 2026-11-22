//! A Debug Adapter Protocol connection: LSP-style frames (`Content-Length`)
//! of DAP messages (`seq`, `type`: request, response or event), over TCP.

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

/// The largest message accepted from an adapter.
const MAX_MESSAGE: usize = 64 << 20;

/// Why a request didn't get a result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DapError {
    Closed,
    TimedOut,
    /// The adapter said no; its words.
    Failed(String),
}

impl std::fmt::Display for DapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => write!(f, "the debugger isn't running"),
            Self::TimedOut => write!(f, "the debugger didn't answer in time"),
            Self::Failed(message) => write!(f, "{message}"),
        }
    }
}

pub fn write_message(out: &mut impl Write, message: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(message)?;
    write!(out, "Content-Length: {}\r\n\r\n", body.len())?;
    out.write_all(&body)?;
    out.flush()
}

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

/// Called with each event (its name and body).
pub type OnEvent = Box<dyn Fn(&str, Value) + Send>;
/// Called with each request the adapter makes of us (its command and
/// arguments); the answer is its response body, or why it failed.
pub type OnRequest = Box<dyn Fn(&str, Value) -> Result<Value, String> + Send>;

type Pending = Arc<Mutex<HashMap<i64, mpsc::Sender<Result<Value, DapError>>>>>;

/// One connection to an adapter.
pub struct DapConnection {
    out: Arc<Mutex<TcpStream>>,
    pending: Pending,
    seq: Arc<AtomicI64>,
    alive: Arc<AtomicBool>,
}

impl DapConnection {
    /// Talks over `stream`; events and the adapter's requests go to the
    /// handlers, on a thread of its own. `on_close` runs once, at the end.
    pub fn new(
        stream: TcpStream,
        on_event: OnEvent,
        on_request: OnRequest,
        on_close: Box<dyn FnOnce() + Send>,
    ) -> io::Result<Self> {
        let reader = stream.try_clone()?;
        let out = Arc::new(Mutex::new(stream));
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let seq = Arc::new(AtomicI64::new(1));
        let alive = Arc::new(AtomicBool::new(true));
        let (reply, read_pending, read_seq, read_alive) = (
            Arc::clone(&out),
            Arc::clone(&pending),
            Arc::clone(&seq),
            Arc::clone(&alive),
        );
        thread::Builder::new()
            .name("ide-debug read".to_owned())
            .spawn(move || {
                let mut input = BufReader::new(reader);
                while let Ok(Some(message)) = read_message(&mut input) {
                    match message.get("type").and_then(Value::as_str) {
                        Some("response") => answer(&message, &read_pending),
                        Some("event") => {
                            let name = message.get("event").and_then(Value::as_str).unwrap_or("");
                            on_event(name, message.get("body").cloned().unwrap_or(Value::Null));
                        }
                        Some("request") => {
                            let command = message
                                .get("command")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_owned();
                            let arguments =
                                message.get("arguments").cloned().unwrap_or(Value::Null);
                            let result = on_request(&command, arguments);
                            let (success, body, error) = match result {
                                Ok(body) => (true, body, None),
                                Err(error) => (false, Value::Null, Some(error)),
                            };
                            let mut response = json!({
                                "seq": read_seq.fetch_add(1, Ordering::Relaxed),
                                "type": "response",
                                "request_seq": message.get("seq").cloned().unwrap_or(Value::Null),
                                "command": command,
                                "success": success,
                                "body": body,
                            });
                            if let Some(error) = error {
                                response["message"] = Value::String(error);
                            }
                            let mut out = reply.lock().unwrap_or_else(PoisonError::into_inner);
                            let _ = write_message(&mut *out, &response);
                        }
                        _ => {}
                    }
                }
                read_alive.store(false, Ordering::Release);
                for (_, waiting) in read_pending
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .drain()
                {
                    let _ = waiting.send(Err(DapError::Closed));
                }
                on_close();
            })?;
        Ok(Self {
            out,
            pending,
            seq,
            alive,
        })
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }

    /// A request, and its response body (or the adapter's reason), within `limit`.
    pub fn request(
        &self,
        command: &str,
        arguments: Value,
        limit: Duration,
    ) -> Result<Value, DapError> {
        if !self.is_alive() {
            return Err(DapError::Closed);
        }
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        let (answer, answered) = mpsc::channel();
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(seq, answer);
        let message =
            json!({ "seq": seq, "type": "request", "command": command, "arguments": arguments });
        let sent = {
            let mut out = self.out.lock().unwrap_or_else(PoisonError::into_inner);
            write_message(&mut *out, &message)
        };
        if sent.is_err() {
            self.pending
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&seq);
            return Err(DapError::Closed);
        }
        match answered.recv_timeout(limit) {
            Ok(result) => result,
            Err(_) => {
                self.pending
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(&seq);
                Err(DapError::TimedOut)
            }
        }
    }

    /// A request whose answer isn't waited for (js-debug answers `launch`
    /// only once it's configured, after requests that come later).
    pub fn fire(&self, command: &str, arguments: Value) {
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        let message =
            json!({ "seq": seq, "type": "request", "command": command, "arguments": arguments });
        let mut out = self.out.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = write_message(&mut *out, &message);
    }

    /// Closes the connection (the adapter sees it end).
    pub fn close(&self) {
        let out = self.out.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = out.shutdown(std::net::Shutdown::Both);
    }
}

fn answer(message: &Value, pending: &Pending) {
    let Some(seq) = message.get("request_seq").and_then(Value::as_i64) else {
        return;
    };
    let Some(waiting) = pending
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&seq)
    else {
        return;
    };
    let result = if message.get("success").and_then(Value::as_bool) == Some(true) {
        Ok(message.get("body").cloned().unwrap_or(Value::Null))
    } else {
        Err(DapError::Failed(
            message
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("the debugger refused")
                .to_owned(),
        ))
    };
    let _ = waiting.send(result);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let mut wire = Vec::new();
        write_message(
            &mut wire,
            &json!({ "seq": 1, "type": "event", "event": "stopped" }),
        )
        .unwrap();
        let mut input = io::Cursor::new(wire);
        assert_eq!(
            read_message(&mut input).unwrap(),
            Some(json!({ "seq": 1, "type": "event", "event": "stopped" }))
        );
        assert_eq!(read_message(&mut input).unwrap(), None);
    }

    #[test]
    fn a_frame_without_a_length_is_an_error() {
        assert!(read_message(&mut io::Cursor::new(b"\r\n{}".to_vec())).is_err());
    }
}
