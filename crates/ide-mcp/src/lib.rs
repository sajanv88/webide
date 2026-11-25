//! An MCP server that lets AI agents work in the folder open in the IDE.
//!
//! Streamable HTTP (the official Rust SDK's transport) on 127.0.0.1 only,
//! answered by hyper on a thread of its own. Every request must carry the
//! token (`Authorization: Bearer …`); a request from a web page (it has an
//! `Origin`) is refused, as is a `Host` that isn't this machine, so a site
//! the person visits can't reach it. Agents read the workspace; they write
//! files in it only when the person allows edits, and never in `.git`.
//! Nothing here runs a command.

mod diff;
mod http;
mod tools;

#[cfg(test)]
mod tests;

use std::fs;
use std::io::{self, Write};
use std::net::{Ipv4Addr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use ide_core::agent::{
    AgentConfig, AgentHost, AgentServer, AgentSink, AgentWorkbench, WorkspaceTools,
};
use ide_core::{GitClient, IdeError, WorkspaceFs, WorkspaceSearch};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

pub use crate::diff::unified_diff;
pub use crate::tools::workspace_relative;

/// The configuration, beside the app's settings.
const CONFIG_FILE: &str = "agents.json";
/// The token, alone in its file.
const TOKEN_FILE: &str = "agent-token";
/// Bytes of randomness in a token (shown as twice as many hex digits).
const TOKEN_BYTES: usize = 32;

/// [`AgentHost`] on the workspace's own ports.
pub struct McpHost {
    dir: PathBuf,
    fs: Arc<dyn WorkspaceFs>,
    search: Arc<dyn WorkspaceSearch>,
    git: Arc<dyn GitClient>,
}

impl std::fmt::Debug for McpHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpHost")
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
}

impl McpHost {
    /// Keeps its configuration and token in `dir` (the app's config folder).
    pub fn new(
        dir: PathBuf,
        fs: Arc<dyn WorkspaceFs>,
        search: Arc<dyn WorkspaceSearch>,
        git: Arc<dyn GitClient>,
    ) -> Self {
        Self {
            dir,
            fs,
            search,
            git,
        }
    }

    fn save_token(&self) -> Result<String, IdeError> {
        let token = new_token()?;
        write_private(&self.dir.join(TOKEN_FILE), token.as_bytes())?;
        Ok(token)
    }

    /// Serves on `listener`, which is already bound.
    fn serve_on(
        &self,
        listener: TcpListener,
        allow_edits: bool,
        token: &str,
        workbench: Arc<dyn AgentWorkbench>,
        sink: AgentSink,
    ) -> Result<Box<dyn AgentServer>, IdeError> {
        let port = listener.local_addr().map_err(io_error)?.port();
        listener.set_nonblocking(true).map_err(io_error)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(io_error)?;
        let tools = tools::Tools::new(tools::Ports {
            fs: Arc::clone(&self.fs),
            search: Arc::clone(&self.search),
            git: Arc::clone(&self.git),
            workbench,
            sink,
            allow_edits,
        });
        let (stop, stopped) = watch::channel(false);
        let token: Arc<str> = Arc::from(token);
        let thread = thread::Builder::new()
            .name("ide-mcp".to_owned())
            .spawn(move || {
                runtime.block_on(http::serve(listener, tools, token, stopped));
                // A tool still working (a long search) isn't waited for.
                runtime.shutdown_timeout(Duration::from_millis(500));
            })
            .map_err(io_error)?;
        Ok(Box::new(Running {
            url: format!("http://127.0.0.1:{port}/mcp"),
            stop,
            thread: Some(thread),
        }))
    }
}

/// The configuration as it's saved.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct SavedConfig {
    enabled: bool,
    allow_edits: bool,
    port: u16,
}

impl Default for SavedConfig {
    fn default() -> Self {
        AgentConfig::default().into()
    }
}

impl From<AgentConfig> for SavedConfig {
    fn from(config: AgentConfig) -> Self {
        Self {
            enabled: config.enabled,
            allow_edits: config.allow_edits,
            port: config.port,
        }
    }
}

impl AgentHost for McpHost {
    fn config(&self) -> Result<AgentConfig, IdeError> {
        let text = match fs::read_to_string(self.dir.join(CONFIG_FILE)) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(AgentConfig::default());
            }
            Err(error) => return Err(io_error(error)),
        };
        // A file that can't be read as ours is taken as the default: off.
        let saved: SavedConfig = serde_json::from_str(&text).unwrap_or_default();
        let config = AgentConfig {
            enabled: saved.enabled,
            allow_edits: saved.allow_edits,
            port: saved.port,
        };
        Ok(if config.validate().is_ok() {
            config
        } else {
            AgentConfig::default()
        })
    }

    fn save_config(&self, config: &AgentConfig) -> Result<(), IdeError> {
        config.validate()?;
        let text = serde_json::to_string_pretty(&SavedConfig::from(*config))
            .map_err(|error| IdeError::Io(error.to_string()))?;
        write_private(&self.dir.join(CONFIG_FILE), text.as_bytes())
    }

    fn token(&self) -> Result<String, IdeError> {
        match fs::read_to_string(self.dir.join(TOKEN_FILE)) {
            Ok(text) if valid_token(text.trim()) => Ok(text.trim().to_owned()),
            Ok(_) => self.save_token(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => self.save_token(),
            Err(error) => Err(io_error(error)),
        }
    }

    fn new_token(&self) -> Result<String, IdeError> {
        self.save_token()
    }

    fn serve(
        &self,
        config: &AgentConfig,
        token: &str,
        workbench: Arc<dyn AgentWorkbench>,
        sink: AgentSink,
    ) -> Result<Box<dyn AgentServer>, IdeError> {
        config.validate()?;
        if !valid_token(token) {
            return Err(IdeError::InvalidArgument("not a valid token".to_owned()));
        }
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, config.port)).map_err(|error| {
            if error.kind() == io::ErrorKind::AddrInUse {
                IdeError::Io(format!(
                    "port {} is in use by another program: choose another",
                    config.port
                ))
            } else {
                io_error(error)
            }
        })?;
        self.serve_on(listener, config.allow_edits, token, workbench, sink)
    }

    fn workspace_tools(&self, workbench: Arc<dyn AgentWorkbench>) -> Arc<dyn WorkspaceTools> {
        Arc::new(tools::Toolbox::new(tools::Ports {
            fs: Arc::clone(&self.fs),
            search: Arc::clone(&self.search),
            git: Arc::clone(&self.git),
            workbench,
            sink: Arc::new(|_| {}),
            allow_edits: false,
        }))
    }
}

/// A running server: dropping it stops it, and waits for its thread.
struct Running {
    url: String,
    stop: watch::Sender<bool>,
    thread: Option<JoinHandle<()>>,
}

impl AgentServer for Running {
    fn url(&self) -> String {
        self.url.clone()
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A new token: random bytes from the OS, as lowercase hex.
fn new_token() -> Result<String, IdeError> {
    let mut bytes = [0_u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes)
        .map_err(|error| IdeError::Io(format!("no randomness for a token: {error}")))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn valid_token(token: &str) -> bool {
    token.len() == TOKEN_BYTES * 2 && token.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Writes `bytes` to `path` through a temporary file, readable only by the
/// person on Unix.
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), IdeError> {
    let dir = path
        .parent()
        .ok_or_else(|| IdeError::Io("no folder for the file".to_owned()))?;
    fs::create_dir_all(dir).map_err(io_error)?;
    let temporary = path.with_extension("tmp");
    {
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary).map_err(io_error)?;
        file.write_all(bytes).map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
    }
    fs::rename(&temporary, path).map_err(io_error)
}

fn io_error(error: io::Error) -> IdeError {
    IdeError::Io(error.to_string())
}
