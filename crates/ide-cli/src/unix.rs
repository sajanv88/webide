//! macOS and Linux: the command is a link to the app's own program, in a
//! folder that's on the PATH.

use std::io::{self, IsTerminal};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use ide_core::IdeError;
use ide_core::cli::CommandLineStatus;

use crate::{COMMAND, LocalCommandLine};

#[cfg(target_os = "linux")]
use crate::linux as platform;
#[cfg(target_os = "macos")]
use crate::macos as platform;

/// Where the link goes when nothing else says: `/usr/local/bin` on macOS,
/// `~/.local/bin` on Linux.
pub fn default_link() -> PathBuf {
    platform::link_folder().join(COMMAND)
}

/// Started from a terminal, the app starts again without it and this copy
/// ends. The new copy has no terminal, so it stays. Release builds only:
/// `tauri dev` runs the app from one and waits for it.
pub fn detach() -> Option<ExitCode> {
    if cfg!(debug_assertions) || !io::stdin().is_terminal() {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let started = Command::new(exe)
        .args(std::env::args_os().skip(1))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // Its own process group: closing the terminal doesn't end it.
        .process_group(0)
        .spawn();
    // If it couldn't start, this copy carries on in the terminal.
    started.ok().map(|_| ExitCode::SUCCESS)
}

pub fn status(line: &LocalCommandLine) -> CommandLineStatus {
    let target = match platform::target(&line.exe) {
        Ok(target) => target,
        Err(status) => return status,
    };
    if std::fs::read_link(&line.link).is_ok_and(|now| now == target) {
        installed(&line.link)
    } else {
        CommandLineStatus::NotInstalled
    }
}

pub fn install(line: &LocalCommandLine) -> Result<CommandLineStatus, IdeError> {
    let target = match platform::target(&line.exe) {
        Ok(target) => target,
        Err(status) => return Ok(status),
    };
    match std::fs::symlink_metadata(&line.link) {
        // Already ours.
        Ok(meta) if meta.is_symlink() && std::fs::read_link(&line.link)? == target => {
            return Ok(installed(&line.link));
        }
        // Someone else's program: not replaced.
        Ok(meta) if !meta.is_symlink() => {
            return Err(IdeError::InvalidArgument(format!(
                "{} is another program: remove it first to put webide's there",
                line.link.display()
            )));
        }
        _ => {}
    }
    match link(&target, &line.link) {
        Ok(()) => Ok(installed(&line.link)),
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            platform::link_as_admin(&target, &line.link)?;
            Ok(installed(&line.link))
        }
        Err(error) => Err(IdeError::from(error)),
    }
}

pub fn uninstall(line: &LocalCommandLine) -> Result<CommandLineStatus, IdeError> {
    let target = match platform::target(&line.exe) {
        Ok(target) => target,
        Err(status) => return Ok(status),
    };
    // Only a link to this app is taken away.
    if std::fs::read_link(&line.link).is_ok_and(|now| now == target) {
        match std::fs::remove_file(&line.link) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                platform::remove_as_admin(&line.link)?;
            }
            Err(error) => return Err(IdeError::from(error)),
        }
    }
    Ok(CommandLineStatus::NotInstalled)
}

/// Makes `link` point at `target`, replacing a link that was there.
fn link(target: &Path, link: &Path) -> io::Result<()> {
    if let Some(folder) = link.parent() {
        std::fs::create_dir_all(folder)?;
    }
    if std::fs::symlink_metadata(link).is_ok_and(|meta| meta.is_symlink()) {
        std::fs::remove_file(link)?;
    }
    std::os::unix::fs::symlink(target, link)
}

/// It's at `link`; says so if that folder isn't on the PATH.
pub fn installed(link: &Path) -> CommandLineStatus {
    let folder = link.parent().unwrap_or(link);
    let on_path = std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|entry| entry == folder));
    CommandLineStatus::Installed {
        path: link.display().to_string(),
        hint: (!on_path).then(|| {
            format!(
                "{} isn't on your PATH: add it in your shell's profile, then open a new terminal.",
                folder.display()
            )
        }),
    }
}
