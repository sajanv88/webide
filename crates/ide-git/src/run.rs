//! Running `git`: straight to the program, never through a shell, with a time
//! limit, no terminal prompts, and on Windows no console window.

use std::ffi::OsStr;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// What a finished `git` run gave.
#[derive(Debug)]
pub struct Output {
    pub stdout: Vec<u8>,
}

/// Why a run didn't succeed.
#[derive(Debug, PartialEq, Eq)]
pub enum RunError {
    /// No `git` on the PATH.
    NotFound,
    /// It ran and failed; what it said on stderr.
    Failed { code: Option<i32>, stderr: String },
    /// It ran past its time and was stopped.
    TimedOut(Duration),
    /// It couldn't be started or read for another reason.
    Io(String),
}

impl RunError {
    /// For people: git's own words where there are some.
    pub fn message(&self) -> String {
        match self {
            Self::NotFound => "git isn't installed, or isn't on the PATH".to_owned(),
            Self::Failed { stderr, code } => {
                let text = stderr.trim();
                if text.is_empty() {
                    format!("git failed (exit code {})", code.unwrap_or(-1))
                } else {
                    text.to_owned()
                }
            }
            Self::TimedOut(limit) => {
                format!("git took longer than {} s and was stopped", limit.as_secs())
            }
            Self::Io(message) => message.clone(),
        }
    }
}

/// Options every run gets, before its own arguments.
const GLOBAL_ARGS: &[&str] = &[
    "--no-pager",
    // Paths are paths, never globs or pathspec magic.
    "--literal-pathspecs",
    "-c",
    "core.quotepath=false",
    "-c",
    "color.ui=false",
    // A configured fsmonitor is a program git runs on each status: never here.
    "-c",
    "core.fsmonitor=false",
];

/// Runs `git <args>` in `cwd`, with `stdin` written to it when given.
pub fn git<S: AsRef<OsStr>>(
    cwd: &Path,
    args: &[S],
    stdin: Option<&[u8]>,
    limit: Duration,
) -> Result<Output, RunError> {
    let mut command = Command::new("git");
    command
        .args(GLOBAL_ARGS)
        .args(args)
        .current_dir(cwd)
        // Never wait on a prompt nobody can see (credentials, an editor).
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EDITOR", "true")
        // Reading the status shouldn't take the index lock from the person's own git.
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: the app has no console, so each run would open one.
        command.creation_flags(0x0800_0000);
    }

    let mut child = command.spawn().map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => RunError::NotFound,
        _ => RunError::Io(format!("couldn't start git: {error}")),
    })?;

    // Read both pipes while it runs, so neither fills up and stalls it.
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let out = thread::spawn(move || read_all(stdout.as_mut()));
    let err = thread::spawn(move || read_all(stderr.as_mut()));
    if let (Some(input), Some(mut pipe)) = (stdin, child.stdin.take()) {
        // A failed write shows up as git's own error.
        let _ = pipe.write_all(input);
    }

    let deadline = Instant::now() + limit;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RunError::TimedOut(limit));
            }
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(error) => return Err(RunError::Io(format!("couldn't wait for git: {error}"))),
        }
    };
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    if status.success() {
        Ok(Output { stdout })
    } else {
        Err(RunError::Failed {
            code: status.code(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        })
    }
}

fn read_all(pipe: Option<&mut impl Read>) -> Vec<u8> {
    let mut bytes = Vec::new();
    if let Some(pipe) = pipe {
        let _ = pipe.read_to_end(&mut bytes);
    }
    bytes
}
