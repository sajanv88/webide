//! The IDE's AI assistant: a conversation with a model the person sets up
//! (their own key or account), which reads the open workspace through the
//! same tools agents get, and proposes changes the person accepts or rejects.
//!
//! Every request to a model goes from the app's own process: the webview
//! never reaches the network.

use std::sync::Arc;

use crate::IdeError;
use crate::agent::WorkspaceTools;

/// Where the model is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderKind {
    /// Claude, with an Anthropic API key.
    Anthropic,
    /// With an OpenAI API key.
    OpenAi,
    /// A deployment in Azure AI Foundry (Azure OpenAI), with its key.
    AzureFoundry,
    /// With a Gemini API key.
    Gemini,
    /// Ollama on this machine or the network; no key.
    Ollama,
    /// Amazon Bedrock, with an AWS profile or a Bedrock API key.
    Bedrock,
}

impl ProviderKind {
    pub const ALL: [Self; 6] = [
        Self::Anthropic,
        Self::OpenAi,
        Self::AzureFoundry,
        Self::Gemini,
        Self::Ollama,
        Self::Bedrock,
    ];

    /// The name it's kept under: `anthropic`, `openai`…
    pub fn id(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAi => "openai",
            Self::AzureFoundry => "azure",
            Self::Gemini => "gemini",
            Self::Ollama => "ollama",
            Self::Bedrock => "bedrock",
        }
    }
}

/// How Bedrock requests are signed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BedrockAuth {
    /// The AWS credentials on this machine, from a profile (SSO included).
    #[default]
    Profile,
    /// A Bedrock API key.
    ApiKey,
}

/// What the person chose in the setup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSetup {
    pub kind: ProviderKind,
    /// The model's id, or for Azure the deployment's name.
    pub model: String,
    /// Azure: the resource's endpoint. Ollama: the server, `http://127.0.0.1:11434` when `None`.
    pub endpoint: Option<String>,
    /// Bedrock: the AWS region.
    pub region: Option<String>,
    /// Bedrock with `BedrockAuth::Profile`: the profile, the default one when `None`.
    pub profile: Option<String>,
    pub bedrock_auth: BedrockAuth,
}

impl ProviderSetup {
    /// Whether this setup needs a secret (an API key) kept for it.
    pub fn needs_secret(&self) -> bool {
        match self.kind {
            ProviderKind::Ollama => false,
            ProviderKind::Bedrock => self.bedrock_auth == BedrockAuth::ApiKey,
            _ => true,
        }
    }
}

/// The assistant's saved setup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantStatus {
    /// `None` until it's set up.
    pub setup: Option<ProviderSetup>,
    /// A key is kept for the setup's provider (in the system's credential store).
    pub has_secret: bool,
}

/// A model to pick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: String,
    /// What the provider calls it, when it says.
    pub name: Option<String>,
}

/// What the assistant is told about where the person is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChatContext {
    /// The open folder's name.
    pub workspace: Option<String>,
    /// The file in front of the person, relative to the folder.
    pub active_path: Option<String>,
}

/// How a tool call went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    Running,
    Done,
    Failed,
}

/// What a conversation reports as it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssistantEvent {
    /// More of the reply's text.
    Text { conversation: String, delta: String },
    /// A read tool started, finished or failed.
    Tool {
        conversation: String,
        call_id: String,
        name: String,
        /// Its arguments, as JSON.
        input: String,
        status: ToolStatus,
        /// The result (shortened) or why it failed, once it's over.
        output: Option<String>,
    },
    /// A change to a file, waiting for the person: `decide` accepts or rejects it.
    Proposal {
        conversation: String,
        call_id: String,
        path: String,
        /// The file's text now; `None` for a new file.
        original: Option<String>,
        proposed: String,
        /// What the change does, in the assistant's words.
        summary: Option<String>,
    },
    /// The person decided; `error` says why an accepted change wasn't written.
    ProposalDecided {
        conversation: String,
        call_id: String,
        accepted: bool,
        error: Option<String>,
    },
    /// The reply is over.
    Done { conversation: String },
    /// The reply stopped: the provider failed, or the person stopped it.
    Failed {
        conversation: String,
        message: String,
    },
}

/// Where conversations report.
pub type AssistantSink = Arc<dyn Fn(AssistantEvent) + Send + Sync>;

/// The assistant: its setup, the secret kept for it, and conversations.
pub trait AssistantHost: Send + Sync {
    fn status(&self) -> Result<AssistantStatus, IdeError>;
    /// Saves the setup, and `secret` for its provider when given (an empty
    /// one is refused). A setup that needs a secret can't be saved without one
    /// kept already.
    fn save(
        &self,
        setup: &ProviderSetup,
        secret: Option<&str>,
    ) -> Result<AssistantStatus, IdeError>;
    /// Forgets the setup and every secret kept.
    fn forget(&self) -> Result<AssistantStatus, IdeError>;
    /// The models the provider offers, with `secret` or the one kept. Blocks.
    fn list_models(
        &self,
        setup: &ProviderSetup,
        secret: Option<&str>,
    ) -> Result<Vec<ModelInfo>, IdeError>;
    /// Asks the model for a word, to check the setup works: its reply. Blocks.
    fn check(&self, setup: &ProviderSetup, secret: Option<&str>) -> Result<String, IdeError>;
    /// Sends the person's message in `conversation` (a new id starts one) and
    /// returns: the reply comes to `sink`. Fails at once when there's no setup,
    /// or a reply is already coming in that conversation.
    fn send(
        &self,
        conversation: &str,
        text: &str,
        context: ChatContext,
        tools: Arc<dyn WorkspaceTools>,
        sink: AssistantSink,
    ) -> Result<(), IdeError>;
    /// Stops the reply coming in `conversation`, if any; a waiting proposal is rejected.
    fn stop(&self, conversation: &str);
    /// Accepts or rejects a waiting proposal.
    fn decide(&self, conversation: &str, call_id: &str, accept: bool) -> Result<(), IdeError>;
    /// Forgets a conversation's messages.
    fn clear(&self, conversation: &str);
}
