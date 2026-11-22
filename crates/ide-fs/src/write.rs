//! Atomic replace: write a temporary file beside the target, flush it, rename it over.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use ide_core::fs::TEMP_FILE_PREFIX;

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) fn atomic(target: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = target
        .parent()
        .ok_or_else(|| io::Error::other("target has no parent folder"))?;
    let temp = dir.join(format!(
        "{TEMP_FILE_PREFIX}{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));

    let result = write_then_rename(&temp, target, bytes);
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn write_then_rename(temp: &Path, target: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    // Keep the original file's permissions (Unix mode bits).
    if let Ok(existing) = fs::metadata(target) {
        fs::set_permissions(temp, existing.permissions())?;
    }
    fs::rename(temp, target)
}
