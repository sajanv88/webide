// Prevents an extra console window on Windows in release builds. Do not remove.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Composition root. It wires crates together and contains no logic.

use std::process::ExitCode;
use std::sync::Arc;

use ide_assistant::AssistantService;
use ide_cli::LocalCommandLine;
use ide_debug::NodeDebugger;
use ide_fs::LocalFs;
use ide_git::LocalGit;
use ide_ipc::Services;
use ide_lsp::{ServerSpec, Servers};
use ide_mcp::McpHost;
use ide_search::LocalSearch;
use ide_terminal::LocalTerminals;
use ide_watcher::LocalWatcher;
use ide_workspace::JsonWorkspaceStore;

fn main() -> ExitCode {
    // The `webide` command: the installer's flags, and the terminal left free.
    let command_line = Arc::new(LocalCommandLine::new(
        std::env::current_exe().unwrap_or_default(),
    ));
    if let Some(code) = ide_cli::run_flag(std::env::args_os().skip(1), command_line.as_ref()) {
        return code;
    }
    if let Some(code) = ide_cli::detach_from_terminal() {
        return code;
    }
    // `webide <folder or file>`, relative to where the terminal is.
    let launch = std::env::current_dir()
        .ok()
        .and_then(|cwd| ide_cli::launch_target(std::env::args_os().skip(1), &cwd));
    let result = ide_ipc::run(tauri::generate_context!(), move |paths| {
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
            command_line,
            launch,
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
            // TypeScript is staged into the bundle by scripts/stage-typescript.mjs;
            // the others (and their schemas), bundled by
            // scripts/stage-language-servers.mjs, run on the person's `node`.
            languages: {
                let servers = paths.resource_dir.join("language-servers");
                let schemas = servers.join("schemas");
                Arc::new(Servers::new(
                    vec![
                        ServerSpec::typescript(
                            paths
                                .resource_dir
                                .join("typescript")
                                .join(if cfg!(windows) { "tsc.exe" } else { "tsc" }),
                        ),
                        ServerSpec::json(servers.join("json-server.cjs"), schemas.clone()),
                        ServerSpec::css(servers.join("css-server.cjs")),
                        // Before HTML: in an Angular project, its templates are Angular's.
                        ServerSpec::angular(servers.join("angular-server.cjs")),
                        ServerSpec::html(servers.join("html-server.cjs")),
                        ServerSpec::yaml(servers.join("yaml-server.mjs"), schemas),
                    ],
                    Arc::new(ide_debug::find_node),
                ))
            },
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
