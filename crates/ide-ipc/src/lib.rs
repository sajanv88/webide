//! The only crate that depends on Tauri.
//!
//! It builds the Tauri app, registers commands, and maps domain types to DTOs
//! (exported to `packages/contracts` with ts-rs: `pnpm contracts:generate`).
//! Feature crates reach it only as `ide-core` ports, handed in by the
//! composition root through [`Services`].

mod commands;
mod dto;
mod error;
mod events;
mod handlers;

#[cfg(test)]
mod export;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::Arc;

use tauri::webview::PageLoadEvent;
use tauri::{Manager, RunEvent, Runtime, WindowEvent};

pub use commands::COMMAND_NAMES;
pub use error::{IpcError, IpcErrorCode};
pub use handlers::Services;

use crate::dto::{AppInfoDto, BackendEventDto, OsDto, WindowStateDto};
use crate::events::EventHub;
use crate::handlers::{Emit, IpcState};

/// Label of the main window, as declared in `tauri.conf.json` and its capability.
pub const MAIN_WINDOW: &str = "main";

/// Where the app may keep its own files, resolved by Tauri for this OS and app identifier.
#[derive(Debug, Clone)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    /// Where the app's bundled files are (the TypeScript language server).
    pub resource_dir: PathBuf,
}

/// When set to an absolute folder, the app keeps its settings, recent folders
/// and the rest there instead of in its own place: a separate profile, for
/// trying a build without touching the one used day to day.
pub const CONFIG_DIR_VAR: &str = "WEBIDE_CONFIG_DIR";

/// The folder [`CONFIG_DIR_VAR`] names, if it's set to an absolute path.
pub fn config_dir_override(value: Option<std::ffi::OsString>) -> Option<PathBuf> {
    let dir = PathBuf::from(value?);
    dir.is_absolute().then_some(dir)
}

/// Builds and runs the app until the last window closes.
///
/// `context` comes from `tauri::generate_context!()`, which must be expanded in
/// the crate that owns `tauri.conf.json`. `services` builds the port
/// implementations once the app's paths are known.
pub fn run<F>(context: tauri::Context<tauri::Wry>, services: F) -> Result<(), tauri::Error>
where
    F: FnOnce(&AppPaths) -> Services + Send + 'static,
{
    let hub = Arc::new(EventHub::default());
    let emit_hub = Arc::clone(&hub);
    let emit: Emit = Arc::new(move |event| {
        emit_hub.send(event);
    });
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(hub)
        .invoke_handler(commands::invoke_handler())
        .on_page_load(|webview, payload| {
            // A reload drops the old page's channel and its unsaved changes.
            if payload.event() == PageLoadEvent::Started
                && let Some(hub) = webview.try_state::<Arc<EventHub>>()
            {
                hub.reset();
            }
            // Its terminals were the old page's: end them.
            if payload.event() == PageLoadEvent::Started
                && let Some(state) = webview.try_state::<Arc<IpcState>>()
            {
                state.close_terminals();
                state.stop_language();
                state.stop_debugging();
            }
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event
                && let Some(hub) = window.try_state::<Arc<EventHub>>()
                && hub.hold_close()
            {
                api.prevent_close();
            }
            // The title bar draws the window's buttons: it follows maximizing.
            if let WindowEvent::Resized(_) = event
                && let Some(hub) = window.try_state::<Arc<EventHub>>()
            {
                let state = window_state(window);
                if hub.window_state_changed(state) {
                    hub.send(BackendEventDto::WindowState { state });
                }
            }
        })
        .setup(move |app| {
            let paths = AppPaths {
                config_dir: match config_dir_override(std::env::var_os(CONFIG_DIR_VAR)) {
                    Some(dir) => dir,
                    None => app.path().app_config_dir()?,
                },
                resource_dir: app.path().resource_dir()?,
            };
            let info = AppInfoDto {
                name: app.package_info().name.clone(),
                version: app.package_info().version.to_string(),
                os: OsDto::current(),
            };
            let state = Arc::new(IpcState::new(services(&paths), info, emit));
            state.start_agents();
            app.manage(state);
            set_main_window_title(app)?;
            Ok(())
        })
        .build(context)?;
    app.run(|app, event| {
        // Quitting the app (Cmd+Q on macOS) asks the page first too, while
        // its window is still there to answer.
        if let RunEvent::ExitRequested {
            code: None, api, ..
        } = &event
            && app.get_webview_window(MAIN_WINDOW).is_some()
            && let Some(hub) = app.try_state::<Arc<EventHub>>()
            && hub.hold_close()
        {
            api.prevent_exit();
        }
    });
    Ok(())
}

/// Whether `window` is maximized or full screen (not when it can't tell).
pub(crate) fn window_state<R: Runtime>(window: &tauri::Window<R>) -> WindowStateDto {
    WindowStateDto {
        maximized: window.is_maximized().unwrap_or(false),
        fullscreen: window.is_fullscreen().unwrap_or(false),
    }
}

/// The window title comes from `productName`, so the name lives in one place.
fn set_main_window_title<R: Runtime>(app: &tauri::App<R>) -> Result<(), tauri::Error> {
    let name = app.package_info().name.clone();
    match app.get_webview_window(MAIN_WINDOW) {
        Some(window) => window.set_title(&name),
        None => Ok(()),
    }
}
