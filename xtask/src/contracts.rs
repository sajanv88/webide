//! `cargo xtask check-contracts`: the committed contracts match what
//! `pnpm contracts:generate` would write now. It runs the same export into a
//! temporary folder and compares every file.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Generated files outside the TypeScript folder, relative to `apps/desktop/src-tauri`.
const TAURI_FILES: &[&[&str]] = &[&["ipc_commands.rs"], &["permissions", "ipc-commands.toml"]];

pub fn differences(root: &Path) -> Result<Vec<String>, String> {
    let temp = std::env::temp_dir().join(format!("ide-contracts-check-{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp);
    let result = generate_and_compare(root, &temp);
    let _ = fs::remove_dir_all(&temp);
    result
}

fn generate_and_compare(root: &Path, temp: &Path) -> Result<Vec<String>, String> {
    let fresh_ts = temp.join("ts");
    let fresh_tauri = temp.join("tauri");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let output = Command::new(cargo)
        .current_dir(root)
        .args([
            "test",
            "--quiet",
            "--package",
            "ide-ipc",
            "--lib",
            "export::export_bindings",
        ])
        .args(["--", "--ignored", "--exact"])
        .env("IDE_CONTRACTS_TS_DIR", &fresh_ts)
        .env("IDE_CONTRACTS_TAURI_DIR", &fresh_tauri)
        .output()
        .map_err(|error| format!("could not run the export: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "the export failed:\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let committed_ts = root
        .join("packages")
        .join("contracts")
        .join("src")
        .join("generated");
    let committed_tauri = root.join("apps").join("desktop").join("src-tauri");
    let mut differences = compare_dirs(&fresh_ts, &committed_ts)?;
    for parts in TAURI_FILES {
        let relative: PathBuf = parts.iter().collect();
        if let Some(difference) = compare_files(
            &fresh_tauri.join(&relative),
            &committed_tauri.join(&relative),
        ) {
            differences.push(difference);
        }
    }
    Ok(differences)
}

fn ts_files(dir: &Path) -> Result<BTreeSet<String>, String> {
    let mut names = BTreeSet::new();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(names),
        Err(error) => return Err(format!("{}: {error}", dir.display())),
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".ts") {
            names.insert(name);
        }
    }
    Ok(names)
}

fn compare_dirs(fresh: &Path, committed: &Path) -> Result<Vec<String>, String> {
    let fresh_names = ts_files(fresh)?;
    let committed_names = ts_files(committed)?;
    let mut differences = Vec::new();
    for name in fresh_names.difference(&committed_names) {
        differences.push(format!("{name} is missing from {}", committed.display()));
    }
    for name in committed_names.difference(&fresh_names) {
        differences.push(format!(
            "{name} in {} is no longer generated",
            committed.display()
        ));
    }
    for name in fresh_names.intersection(&committed_names) {
        if let Some(difference) = compare_files(&fresh.join(name), &committed.join(name)) {
            differences.push(difference);
        }
    }
    Ok(differences)
}

/// Compares text with line endings normalised, so a CRLF checkout doesn't count.
fn compare_files(fresh: &Path, committed: &Path) -> Option<String> {
    let read = |path: &Path| {
        fs::read_to_string(path)
            .ok()
            .map(|text| text.replace("\r\n", "\n"))
    };
    match (read(fresh), read(committed)) {
        (Some(a), Some(b)) if a == b => None,
        (Some(_), Some(_)) => Some(format!("{} is out of date", committed.display())),
        (Some(_), None) => Some(format!("{} is missing", committed.display())),
        (None, _) => Some(format!("the export didn't write {}", fresh.display())),
    }
}
