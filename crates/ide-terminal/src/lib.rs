//! Terminals on the system's pseudo-terminal (ConPTY on Windows, a pty on
//! macOS and Linux), through `portable-pty`.
//!
//! The shell is the person's default one, started in the workspace folder,
//! with the app's environment, `TERM=xterm-256color` and
//! `COLORTERM=truecolor`. The webview never chooses the program, its
//! arguments or its folder.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use ide_core::terminal::Utf8Stream;
use ide_core::{
    IdeError, TerminalEvent, TerminalHost, TerminalSession, TerminalSink, TerminalSize,
};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};

#[cfg(test)]
mod tests;

/// How much is read from the terminal at once.
const READ_CHUNK: usize = 64 * 1024;
/// After the shell exits, how long its last output may take to arrive.
const DRAIN: Duration = Duration::from_millis(300);

/// [`TerminalHost`] on the local system.
#[derive(Debug, Clone, Default)]
pub struct LocalTerminals {
    /// The shell, when not the person's default (tests).
    shell: Option<(PathBuf, Vec<String>)>,
}

impl LocalTerminals {
    pub fn new() -> Self {
        Self::default()
    }

    /// Runs `program` with `args` instead of the default shell.
    pub fn with_shell(program: impl Into<PathBuf>, args: &[&str]) -> Self {
        Self {
            shell: Some((
                program.into(),
                args.iter().map(|&arg| arg.to_owned()).collect(),
            )),
        }
    }
}

/// The person's default shell, and its arguments.
pub fn default_shell() -> (PathBuf, Vec<String>) {
    if cfg!(windows) {
        // PowerShell 7 when installed, else Windows PowerShell, which always is.
        let program = find_on_path("pwsh.exe").unwrap_or_else(|| PathBuf::from("powershell.exe"));
        return (program, vec!["-NoLogo".to_owned()]);
    }
    let program = std::env::var_os("SHELL")
        .filter(|shell| !shell.is_empty())
        .map(PathBuf::from)
        .filter(|shell| shell.is_file())
        .unwrap_or_else(|| PathBuf::from("/bin/sh"));
    // On macOS, an app started from the Dock gets a bare PATH: a login shell
    // reads the person's profile, as Terminal.app does.
    let login = cfg!(target_os = "macos") && program.file_name().is_some_and(|name| name != "sh");
    (
        program,
        if login {
            vec!["-l".to_owned()]
        } else {
            Vec::new()
        },
    )
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

fn failed(what: &str, error: impl std::fmt::Display) -> IdeError {
    IdeError::Io(format!("{what}: {error}"))
}

fn pty_size(size: TerminalSize) -> PtySize {
    PtySize {
        rows: size.rows,
        cols: size.cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

impl TerminalHost for LocalTerminals {
    fn spawn(
        &self,
        cwd: &Path,
        size: TerminalSize,
        sink: TerminalSink,
    ) -> Result<Box<dyn TerminalSession>, IdeError> {
        let (program, args) = self.shell.clone().unwrap_or_else(default_shell);
        let pair = native_pty_system()
            .openpty(pty_size(size))
            .map_err(|error| failed("couldn't open a terminal", error))?;
        let mut command = CommandBuilder::new(&program);
        command.args(&args);
        command.cwd(cwd);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        let mut child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| failed(&format!("couldn't start {}", program.display()), error))?;
        // Ours closed, so the terminal ends when the shell does.
        drop(pair.slave);
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| failed("couldn't read the terminal", error))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| failed("couldn't write to the terminal", error))?;
        let killer = child.clone_killer();
        let master = Arc::new(Mutex::new(Some(pair.master)));

        // Output, as it comes.
        let (read_done, read_finished) = mpsc::channel::<()>();
        let out = Arc::clone(&sink);
        thread::Builder::new()
            .name("ide-terminal read".to_owned())
            .spawn(move || {
                read_loop(reader, &out);
                let _ = read_done.send(());
            })
            .map_err(|error| failed("couldn't start reading the terminal", error))?;

        // The shell's end: let its last output arrive, close the terminal
        // (ConPTY keeps the pipe open until then), and say so.
        let closing = Arc::clone(&master);
        thread::Builder::new()
            .name("ide-terminal wait".to_owned())
            .spawn(move || {
                let code = child.wait().ok().map(|status| status.exit_code());
                if read_finished.recv_timeout(DRAIN).is_err() {
                    closing
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .take();
                    let _ = read_finished.recv_timeout(DRAIN * 3);
                }
                sink(TerminalEvent::Exited { code });
            })
            .map_err(|error| failed("couldn't watch the shell", error))?;

        let shell = program
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| program.display().to_string());
        Ok(Box::new(Session {
            master,
            writer: Mutex::new(writer),
            killer: Mutex::new(killer),
            shell,
        }))
    }
}

fn read_loop(mut reader: Box<dyn Read + Send>, sink: &TerminalSink) {
    let mut buffer = vec![0u8; READ_CHUNK];
    let mut text = Utf8Stream::new();
    loop {
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                let chunk = text.push(&buffer[..read]);
                if !chunk.is_empty() {
                    sink(TerminalEvent::Output(chunk));
                }
            }
        }
    }
    let rest = text.finish();
    if !rest.is_empty() {
        sink(TerminalEvent::Output(rest));
    }
}

struct Session {
    master: Arc<Mutex<Option<Box<dyn MasterPty + Send>>>>,
    writer: Mutex<Box<dyn Write + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    shell: String,
}

impl TerminalSession for Session {
    fn write(&self, data: &[u8]) -> Result<(), IdeError> {
        let mut writer = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
        writer
            .write_all(data)
            .and_then(|()| writer.flush())
            .map_err(|error| failed("couldn't write to the terminal", error))
    }

    fn resize(&self, size: TerminalSize) -> Result<(), IdeError> {
        match self
            .master
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
        {
            Some(master) => master
                .resize(pty_size(size))
                .map_err(|error| failed("couldn't resize the terminal", error)),
            // The shell has ended.
            None => Ok(()),
        }
    }

    fn shell(&self) -> &str {
        &self.shell
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Ends the shell (and what it runs in the foreground); the wait
        // thread then closes the terminal and reports the exit.
        let _ = self
            .killer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .kill();
    }
}
