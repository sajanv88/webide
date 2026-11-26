//! Linux: a link in `~/.local/bin`, to the AppImage when the app is one. A
//! `.deb` or `.rpm` puts the program in `/usr/bin` itself.

use std::path::{Path, PathBuf};

use ide_core::IdeError;
use ide_core::cli::CommandLineStatus;

pub fn link_folder() -> PathBuf {
    std::env::var_os("HOME")
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
        .join(".local")
        .join("bin")
}

/// What the link points at: the AppImage the app runs from (its program
/// is in a folder mounted only while it runs), or the program. An installed
/// package's program is on the PATH already.
pub fn target(exe: &Path) -> Result<PathBuf, CommandLineStatus> {
    if exe.starts_with("/usr") {
        return Err(CommandLineStatus::Installed {
            path: exe.display().to_string(),
            hint: Some(
                "It came with the app's package, which takes it away when it's removed.".to_owned(),
            ),
        });
    }
    match std::env::var_os("APPIMAGE").map(PathBuf::from) {
        Some(image) if image.is_absolute() => Ok(image),
        _ => Ok(exe.to_path_buf()),
    }
}

/// `~/.local/bin` is the person's own: there's no one to ask.
pub fn link_as_admin(_target: &Path, link: &Path) -> Result<(), IdeError> {
    Err(IdeError::InvalidArgument(format!(
        "{} can't be written: check its folder's permissions",
        link.display()
    )))
}

pub fn remove_as_admin(link: &Path) -> Result<(), IdeError> {
    link_as_admin(link, link)
}
