// Prevents an extra console window on Windows in release builds. Do not remove.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Composition root. It wires crates together and contains no logic.

use std::process::ExitCode;
use std::sync::Arc;

use ide_fs::LocalFs;
use ide_ipc::Services;
use ide_watcher::LocalWatcher;
use ide_workspace::JsonWorkspaceStore;

fn main() -> ExitCode {
    let result = ide_ipc::run(tauri::generate_context!(), |paths| Services {
        fs: Arc::new(LocalFs::new()),
        workspace: Arc::new(JsonWorkspaceStore::new(paths.config_dir.clone())),
        watcher: Arc::new(LocalWatcher::new()),
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("fatal: {error}");
            ExitCode::FAILURE
        }
    }
}
