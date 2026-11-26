//! The `webide` command, so developers can start the app from a terminal:
//! `webide .`, `webide ~/projects/app`, `webide README.md`.
//!
//! - [`launch_target`] reads what the command line asks to open.
//! - [`detach_from_terminal`] lets the terminal go on (macOS and Linux, where
//!   the command is the app's own program).
//! - [`LocalCommandLine`] puts the command on the PATH, or takes it off:
//!   `bin\webide.cmd` beside the app in the user's PATH on Windows, a link in
//!   `/usr/local/bin` on macOS, and in `~/.local/bin` on Linux (a `.deb` or
//!   `.rpm` puts the program in `/usr/bin` itself).
//! - [`run_flag`] does either for the Windows installer:
//!   `webide.exe --install-command-line` and `--uninstall-command-line`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ide_core::IdeError;
use ide_core::cli::{CommandLine, CommandLineStatus, Launch};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
use unix as os;
#[cfg(windows)]
use windows as os;

#[cfg(test)]
mod tests;

/// The command's name, in every terminal.
pub const COMMAND: &str = "webide";

/// What the arguments after the program's name ask to open: the first one
/// that isn't a flag, relative to `cwd`. A folder opens as the workspace; a
/// file opens in its folder. `None` when nothing is named, or what's named
/// isn't there.
pub fn launch_target<I>(args: I, cwd: &Path) -> Option<Launch>
where
    I: IntoIterator<Item = OsString>,
{
    // macOS can add `-psn_…` when the app starts from the Finder; flags aren't paths.
    let named = args
        .into_iter()
        .find(|arg| !arg.to_string_lossy().starts_with('-'))?;
    let path = std::path::absolute(cwd.join(named)).ok()?;
    if path.is_dir() {
        return Some(Launch {
            folder: path,
            file: None,
        });
    }
    if path.is_file() {
        let folder = path.parent()?.to_path_buf();
        return Some(Launch {
            folder,
            file: Some(path),
        });
    }
    None
}

/// Started from a terminal (`webide .`), the app starts again on its own and
/// this copy ends, so the terminal is free at once, as it is with the
/// system's own apps. It's left alone when nothing is a terminal (the
/// Finder, a menu, the copy started here). Windows needs none of this: the
/// app has no console there.
#[cfg(unix)]
pub fn detach_from_terminal() -> Option<ExitCode> {
    unix::detach()
}

/// Windows needs none: the app has no console.
#[cfg(not(unix))]
pub const fn detach_from_terminal() -> Option<ExitCode> {
    None
}

/// What the Windows installer runs: `--install-command-line` puts the
/// command on the PATH, `--uninstall-command-line` takes it off, and the
/// program then ends without opening a window. `None` for any other start.
pub fn run_flag<I>(args: I, command_line: &dyn CommandLine) -> Option<ExitCode>
where
    I: IntoIterator<Item = OsString>,
{
    let flag = args.into_iter().next()?;
    let done = match flag.to_str()? {
        "--install-command-line" => command_line.install(),
        "--uninstall-command-line" => command_line.uninstall(),
        _ => return None,
    };
    Some(match done {
        Ok(_) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    })
}

/// The `webide` command of this copy of the app, whose program is `exe`.
#[derive(Debug, Clone)]
pub struct LocalCommandLine {
    exe: PathBuf,
    /// Where the link goes, on macOS and Linux.
    #[cfg(unix)]
    link: PathBuf,
}

impl LocalCommandLine {
    /// For the program at `exe` (`std::env::current_exe()`).
    pub fn new(exe: PathBuf) -> Self {
        Self {
            #[cfg(unix)]
            link: unix::default_link(),
            exe,
        }
    }

    /// With the link somewhere else: for tests.
    #[cfg(unix)]
    pub fn with_link(exe: PathBuf, link: PathBuf) -> Self {
        Self { exe, link }
    }
}

impl CommandLine for LocalCommandLine {
    fn status(&self) -> CommandLineStatus {
        os::status(self)
    }

    fn install(&self) -> Result<CommandLineStatus, IdeError> {
        os::install(self)
    }

    fn uninstall(&self) -> Result<CommandLineStatus, IdeError> {
        os::uninstall(self)
    }
}
