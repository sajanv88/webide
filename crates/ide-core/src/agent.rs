//! Letting AI agents work in the open workspace, through a local MCP server.
//!
//! Off until the person turns it on. Agents connect to 127.0.0.1 with a
//! token; they can read the workspace, and write files in it only when the
//! person allows edits too. They never run commands.

use std::sync::Arc;

use crate::IdeError;
use crate::fs::ReadLimits;
use crate::workspace::WorkspaceRoot;

/// The port agents connect to, until the person picks another.
pub const DEFAULT_AGENT_PORT: u16 = 7341;
/// The lowest port that may be chosen: the ones below need privileges.
pub const MIN_AGENT_PORT: u16 = 1024;

/// What the person chose for agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentConfig {
    /// The server runs.
    pub enabled: bool,
    /// Agents may write files in the workspace, not only read them.
    pub allow_edits: bool,
    pub port: u16,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            allow_edits: false,
            port: DEFAULT_AGENT_PORT,
        }
    }
}

impl AgentConfig {
    pub fn validate(&self) -> Result<(), IdeError> {
        if self.port < MIN_AGENT_PORT {
            return Err(IdeError::InvalidSettings(format!(
                "the port must be at least {MIN_AGENT_PORT}"
            )));
        }
        Ok(())
    }
}

/// One thing an agent did, for the person to see.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentCall {
    /// The tool's name: `read_file`, `write_file`…
    pub tool: String,
    /// What it was about: a path, or a search's pattern.
    pub target: Option<String>,
    /// Why it failed, if it did.
    pub error: Option<String>,
}

/// Where the server reports each call.
pub type AgentSink = Arc<dyn Fn(AgentCall) + Send + Sync>;

/// What the server reads from the app as it answers.
pub trait AgentWorkbench: Send + Sync {
    /// The folder open now, if any.
    fn root(&self) -> Option<WorkspaceRoot>;
    /// The sizes above which files aren't read, from the settings.
    fn read_limits(&self) -> ReadLimits;
}

/// The agent server: its saved configuration and token, and starting it.
pub trait AgentHost: Send + Sync {
    /// The saved configuration, or the default.
    fn config(&self) -> Result<AgentConfig, IdeError>;
    fn save_config(&self, config: &AgentConfig) -> Result<(), IdeError>;
    /// The token agents must send, made the first time it's asked for.
    fn token(&self) -> Result<String, IdeError>;
    /// Replaces the token: agents using the old one are refused.
    fn new_token(&self) -> Result<String, IdeError>;
    /// Starts serving on `config.port`, answering with what `workbench` says
    /// is open. Fails at once when the port can't be used.
    fn serve(
        &self,
        config: &AgentConfig,
        token: &str,
        workbench: Arc<dyn AgentWorkbench>,
        sink: AgentSink,
    ) -> Result<Box<dyn AgentServer>, IdeError>;
}

/// A running server. Dropping it stops it.
pub trait AgentServer: Send + Sync {
    /// Where agents connect: `http://127.0.0.1:7341/mcp`.
    fn url(&self) -> String;
}
