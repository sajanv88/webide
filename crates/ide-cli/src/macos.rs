//! macOS: a link in `/usr/local/bin`, which may need an administrator's
//! password (the system asks, as it does for VS Code's `code`).

use std::path::{Path, PathBuf};
use std::process::Command;

use ide_core::IdeError;
use ide_core::cli::CommandLineStatus;

pub fn link_folder() -> PathBuf {
    PathBuf::from("/usr/local/bin")
}

/// The app's program, unless it's running from where it won't stay: the disk
/// image, or the copy macOS runs apps from before they're moved.
pub fn target(exe: &Path) -> Result<PathBuf, CommandLineStatus> {
    let shown = exe.to_string_lossy();
    if shown.starts_with("/Volumes/") || shown.contains("/AppTranslocation/") {
        return Err(CommandLineStatus::Unavailable {
            reason: "Move webide to Applications first: it's running from where it won't stay."
                .to_owned(),
        });
    }
    Ok(exe.to_path_buf())
}

/// Asks for an administrator's password to make the link. The paths go to
/// AppleScript as arguments, quoted there for the shell.
pub fn link_as_admin(target: &Path, link: &Path) -> Result<(), IdeError> {
    let folder = link.parent().unwrap_or(link);
    as_admin(
        r#"do shell script "mkdir -p " & quoted form of (item 3 of argv) & " && ln -sfn " & quoted form of (item 1 of argv) & " " & quoted form of (item 2 of argv) with administrator privileges"#,
        &[target, link, folder],
    )
}

pub fn remove_as_admin(link: &Path) -> Result<(), IdeError> {
    as_admin(
        r#"do shell script "rm -f " & quoted form of (item 1 of argv) with administrator privileges"#,
        &[link],
    )
}

fn as_admin(script: &str, args: &[&Path]) -> Result<(), IdeError> {
    let output = Command::new("/usr/bin/osascript")
        .args(["-e", "on run argv", "-e", script, "-e", "end run"])
        .args(args)
        .output()?;
    if output.status.success() {
        Ok(())
    } else if String::from_utf8_lossy(&output.stderr).contains("-128") {
        Err(IdeError::PermissionDenied)
    } else {
        Err(IdeError::Io(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ))
    }
}
