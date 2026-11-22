// Prevents an extra console window on Windows in release builds. Do not remove.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Composition root. It wires crates together and contains no logic.

use std::process::ExitCode;
use std::sync::Arc;

use ide_debug::NodeDebugger;
use ide_fs::LocalFs;
use ide_git::LocalGit;
use ide_ipc::Services;
use ide_lsp::TypeScriptServers;
use ide_search::LocalSearch;
use ide_terminal::LocalTerminals;
use ide_watcher::LocalWatcher;
use ide_workspace::JsonWorkspaceStore;

fn main() -> ExitCode {
    let result = ide_ipc::run(tauri::generate_context!(), |paths| Services {
        fs: Arc::new(LocalFs::new()),
        workspace: Arc::new(JsonWorkspaceStore::new(paths.config_dir.clone())),
        watcher: Arc::new(LocalWatcher::new()),
        search: Arc::new(LocalSearch::new()),
        git: Arc::new(LocalGit::new()),
        terminals: Arc::new(LocalTerminals::new()),
        // Staged into the bundle by scripts/stage-typescript.mjs.
        // Staged into the bundle by scripts/stage-js-debug.mjs; runs on the person's `node`.
        debuggers: Arc::new(NodeDebugger::new(
            paths
                .resource_dir
                .join("js-debug")
                .join("src")
                .join("dapDebugServer.js"),
        )),
        languages: Arc::new(TypeScriptServers::new(
            paths
                .resource_dir
                .join("typescript")
                .join(if cfg!(windows) { "tsc.exe" } else { "tsc" }),
        )),
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("fatal: {error}");
            ExitCode::FAILURE
        }
    }
}
