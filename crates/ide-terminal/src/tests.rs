//! On the real system: a pseudo-terminal and a shell.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ide_core::{TerminalEvent, TerminalHost, TerminalSession, TerminalSize};

use crate::{LocalTerminals, default_shell};

fn size() -> TerminalSize {
    TerminalSize::new(80, 24).unwrap()
}

/// A sink that collects output and the exit.
fn sink() -> (ide_core::TerminalSink, mpsc::Receiver<TerminalEvent>) {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    (
        Arc::new(move |event| {
            let _ = tx.lock().unwrap().send(event);
        }),
        rx,
    )
}

/// The question ConPTY asks when it starts: where's the cursor?
const CURSOR_QUERY: &str = "\u{1b}[6n";
/// The answer a terminal gives: row 1, column 1.
const CURSOR_ANSWER: &[u8] = b"\x1b[1;1R";

/// Everything until the exit (or the time limit): the output, and the code.
/// It answers ConPTY's cursor question, as the app's terminal does.
fn until_exit(
    events: &mpsc::Receiver<TerminalEvent>,
    session: Option<&dyn TerminalSession>,
    limit: Duration,
) -> (String, Option<Option<u32>>) {
    let deadline = Instant::now() + limit;
    let mut output = String::new();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match events.recv_timeout(left) {
            Ok(TerminalEvent::Output(text)) => {
                if text.contains(CURSOR_QUERY)
                    && let Some(session) = session
                {
                    let _ = session.write(CURSOR_ANSWER);
                }
                output.push_str(&text);
            }
            Ok(TerminalEvent::Exited { code }) => return (output, Some(code)),
            Err(_) => break,
        }
    }
    (output, None)
}

fn one_command(script: &str) -> LocalTerminals {
    if cfg!(windows) {
        LocalTerminals::with_shell("cmd.exe", &["/d", "/c", script])
    } else {
        LocalTerminals::with_shell("/bin/sh", &["-c", script])
    }
}

fn interactive() -> LocalTerminals {
    if cfg!(windows) {
        LocalTerminals::with_shell("cmd.exe", &["/d", "/q"])
    } else {
        LocalTerminals::with_shell("/bin/sh", &[])
    }
}

#[test]
fn runs_in_the_folder_given_and_reports_output_then_the_exit() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("marker-file.txt"), "x").unwrap();
    let host = one_command(if cfg!(windows) { "dir /b" } else { "ls" });
    let (sink, events) = sink();
    let session = host.spawn(dir.path(), size(), sink).unwrap();
    let (output, code) = until_exit(&events, Some(session.as_ref()), Duration::from_secs(20));
    assert!(output.contains("marker-file.txt"), "{output:?}");
    assert_eq!(code, Some(Some(0)));
}

#[test]
fn the_exit_code_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let (sink, events) = sink();
    let session = one_command("exit 3")
        .spawn(dir.path(), size(), sink)
        .unwrap();
    let (_, code) = until_exit(&events, Some(session.as_ref()), Duration::from_secs(20));
    assert_eq!(code, Some(Some(3)));
}

#[test]
fn what_is_written_reaches_the_shell() {
    let dir = tempfile::tempdir().unwrap();
    let (sink, events) = sink();
    let session = interactive().spawn(dir.path(), size(), sink).unwrap();
    session.write(b"echo typed-in-here\r").unwrap();
    session.resize(TerminalSize::new(100, 30).unwrap()).unwrap();
    session.write(b"exit\r").unwrap();
    let (output, code) = until_exit(&events, Some(session.as_ref()), Duration::from_secs(20));
    assert!(output.contains("typed-in-here"), "{output:?}");
    assert!(code.is_some(), "the shell didn't exit: {output:?}");
}

#[test]
fn dropping_the_session_ends_the_shell() {
    let dir = tempfile::tempdir().unwrap();
    let (sink, events) = sink();
    let session = interactive().spawn(dir.path(), size(), sink).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let started = Instant::now();
    drop(session);
    let (_, code) = until_exit(&events, None, Duration::from_secs(10));
    assert!(code.is_some(), "no exit after the drop");
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn the_default_shell_is_a_real_program() {
    let (program, _) = default_shell();
    let name = program
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_lowercase();
    if cfg!(windows) {
        assert!(name == "pwsh.exe" || name == "powershell.exe", "{name}");
    } else {
        assert!(program.is_absolute(), "{}", program.display());
    }
}
