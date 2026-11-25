// Prevents an extra console window on Windows in release builds. Do not remove.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Composition root. It wires crates together and contains no logic.

use std::process::ExitCode;
use std::sync::Arc;

use ide_assistant::AssistantService;
use ide_debug::NodeDebugger;
use ide_fs::LocalFs;
use ide_git::LocalGit;
use ide_ipc::Services;
use ide_lsp::TypeScriptServers;
use ide_mcp::McpHost;
use ide_search::LocalSearch;
use ide_terminal::LocalTerminals;
use ide_watcher::LocalWatcher;
use ide_workspace::JsonWorkspaceStore;

fn main() -> ExitCode {
    let result = ide_ipc::run(tauri::generate_context!(), |paths| {
        let fs = Arc::new(LocalFs::new());
        let search = Arc::new(LocalSearch::new());
        let git = Arc::new(LocalGit::new());
        Services {
            // The MCP server for AI agents works through the same ports as the app.
            agents: Arc::new(McpHost::new(
                paths.config_dir.clone(),
                fs.clone(),
                search.clone(),
                git.clone(),
            )),
            // The AI assistant: its setup beside the settings, keys in the
            // system's credential store; it reads through the agents' tools.
            assistant: Arc::new(AssistantService::new(paths.config_dir.clone())),
            fs,
            workspace: Arc::new(JsonWorkspaceStore::new(paths.config_dir.clone())),
            watcher: Arc::new(LocalWatcher::new()),
            search,
            git,
            terminals: Arc::new(LocalTerminals::new()),
            // Staged into the bundle by scripts/stage-js-debug.mjs; runs on the person's `node`.
            debuggers: Arc::new(NodeDebugger::new(
                paths
                    .resource_dir
                    .join("js-debug")
                    .join("src")
                    .join("dapDebugServer.js"),
            )),
            // Staged into the bundle by scripts/stage-typescript.mjs.
            languages: Arc::new(TypeScriptServers::new(
                paths
                    .resource_dir
                    .join("typescript")
                    .join(if cfg!(windows) { "tsc.exe" } else { "tsc" }),
            )),
        }
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("fatal: {error}");
            ExitCode::FAILURE
        }
    }
}
