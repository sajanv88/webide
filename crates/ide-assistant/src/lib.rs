//! The IDE's AI assistant: conversations with a model from Anthropic, OpenAI,
//! Azure AI Foundry, Gemini, Ollama or Amazon Bedrock.
//!
//! Every request goes from this process (the webview never reaches the
//! network), over HTTPS with rustls; Ollama may be plain HTTP on the person's
//! network. API keys are kept in the system's credential store; the setup
//! (provider, model, endpoint, region) in `assistant.json`. The model reads
//! the workspace through `WorkspaceTools`, the same read tools agents get over
//! MCP, and proposes changes, which are written only when the person accepts
//! them. It runs no commands.

mod config;
mod conversation;
mod providers;
mod secrets;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use ide_core::IdeError;
use ide_core::agent::WorkspaceTools;
use ide_core::assistant::{
    AssistantHost, AssistantSink, AssistantStatus, ChatContext, ModelInfo, ProviderKind,
    ProviderSetup,
};
use tokio::runtime::Handle;
use tokio::sync::{oneshot, watch};

use crate::conversation::{Chat, Turn};
use crate::providers::{Connection, Message, Part, Request, Role};
use crate::secrets::{Keyring, Secrets};

/// How long the setup's check waits for the model's word.
const CHECK_TIMEOUT: Duration = Duration::from_secs(90);
/// How long connecting may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// How long the stream may say nothing (a model thinking, a tool's result being read).
const READ_TIMEOUT: Duration = Duration::from_secs(300);

/// [`AssistantHost`] on a tokio runtime of its own thread.
pub struct AssistantService {
    dir: PathBuf,
    secrets: Arc<dyn Secrets>,
    /// The runtime and the HTTP client, or why they couldn't start: every
    /// call then says so.
    engine: Result<Engine, String>,
    chats: Mutex<HashMap<String, Arc<Chat>>>,
}

/// The assistant's runtime, on its thread, and its HTTP client.
struct Engine {
    http: reqwest::Client,
    handle: Handle,
    stop: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Engine {
    fn start() -> Result<Self, String> {
        let failed =
            |error: &dyn std::fmt::Display| format!("the assistant couldn't start: {error}");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| failed(&error))?;
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .read_timeout(READ_TIMEOUT)
            .user_agent(concat!("webide/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| failed(&error))?;
        let handle = runtime.handle().clone();
        let (stop, stopped) = oneshot::channel::<()>();
        let thread = thread::Builder::new()
            .name("assistant".to_owned())
            .spawn(move || {
                runtime.block_on(async {
                    let _ = stopped.await;
                });
                runtime.shutdown_timeout(Duration::from_millis(500));
            })
            .map_err(|error| failed(&error))?;
        Ok(Self {
            http,
            handle,
            stop: Some(stop),
            thread: Some(thread),
        })
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl std::fmt::Debug for AssistantService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AssistantService")
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
}

impl AssistantService {
    /// Keeps its setup in `dir` (the app's config folder) and keys in the
    /// system's credential store.
    pub fn new(dir: PathBuf) -> Self {
        Self::with_secrets(dir, Arc::new(Keyring))
    }

    pub(crate) fn with_secrets(dir: PathBuf, secrets: Arc<dyn Secrets>) -> Self {
        Self {
            dir,
            secrets,
            engine: Engine::start(),
            chats: Mutex::new(HashMap::new()),
        }
    }

    fn engine(&self) -> Result<&Engine, IdeError> {
        self.engine
            .as_ref()
            .map_err(|error| IdeError::Io(error.clone()))
    }

    /// Runs `work` on the assistant's runtime and waits for it.
    fn wait<T: Send + 'static>(
        &self,
        work: impl Future<Output = Result<T, String>> + Send + 'static,
    ) -> Result<T, IdeError> {
        let (done, result) = oneshot::channel();
        self.engine()?.handle.spawn(async move {
            let _ = done.send(work.await);
        });
        result
            .blocking_recv()
            .map_err(|_| IdeError::Io("the assistant stopped".to_owned()))?
            .map_err(IdeError::Io)
    }

    /// `setup` with its secret: `given`, or the one kept.
    fn connection(
        &self,
        setup: &ProviderSetup,
        given: Option<&str>,
    ) -> Result<Connection, IdeError> {
        config::validate(setup)?;
        let secret = match given.map(str::trim).filter(|secret| !secret.is_empty()) {
            Some(secret) => Some(secret.to_owned()),
            None if setup.needs_secret() => self.secrets.get(setup.kind).map_err(IdeError::Io)?,
            None => None,
        };
        if setup.needs_secret() && secret.is_none() {
            return Err(IdeError::InvalidArgument("enter the API key".to_owned()));
        }
        Ok(Connection {
            setup: normalized(setup),
            secret,
            http: self.engine()?.http.clone(),
        })
    }

    fn chat(&self, conversation: &str) -> Option<Arc<Chat>> {
        self.chats.lock().ok()?.get(conversation).cloned()
    }
}

/// `setup` with its text fields trimmed, and empty ones as `None`.
fn normalized(setup: &ProviderSetup) -> ProviderSetup {
    let clean = |value: &Option<String>| {
        value
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    ProviderSetup {
        kind: setup.kind,
        model: setup.model.trim().to_owned(),
        endpoint: clean(&setup.endpoint),
        region: clean(&setup.region),
        profile: clean(&setup.profile),
        bedrock_auth: setup.bedrock_auth,
    }
}

impl AssistantHost for AssistantService {
    fn status(&self) -> Result<AssistantStatus, IdeError> {
        let setup = config::read(&self.dir)?;
        let has_secret = match &setup {
            Some(setup) if setup.needs_secret() => self
                .secrets
                .get(setup.kind)
                .map_err(IdeError::Io)?
                .is_some(),
            _ => false,
        };
        Ok(AssistantStatus { setup, has_secret })
    }

    fn save(
        &self,
        setup: &ProviderSetup,
        secret: Option<&str>,
    ) -> Result<AssistantStatus, IdeError> {
        let setup = normalized(setup);
        config::validate(&setup)?;
        match secret {
            Some(secret) if secret.trim().is_empty() => {
                return Err(IdeError::InvalidArgument("enter the API key".to_owned()));
            }
            Some(secret) if setup.needs_secret() => {
                self.secrets
                    .set(setup.kind, secret.trim())
                    .map_err(IdeError::Io)?;
            }
            _ => {}
        }
        if setup.needs_secret()
            && self
                .secrets
                .get(setup.kind)
                .map_err(IdeError::Io)?
                .is_none()
        {
            return Err(IdeError::InvalidArgument("enter the API key".to_owned()));
        }
        config::write(&self.dir, &setup)?;
        self.status()
    }

    fn forget(&self) -> Result<AssistantStatus, IdeError> {
        for kind in ProviderKind::ALL {
            self.secrets.delete(kind).map_err(IdeError::Io)?;
        }
        config::remove(&self.dir)?;
        self.status()
    }

    fn list_models(
        &self,
        setup: &ProviderSetup,
        secret: Option<&str>,
    ) -> Result<Vec<ModelInfo>, IdeError> {
        // The model isn't chosen yet: any name passes the setup's check.
        let mut setup = setup.clone();
        if setup.model.trim().is_empty() {
            "-".clone_into(&mut setup.model);
        }
        let connection = self.connection(&setup, secret)?;
        self.wait(async move { providers::list_models(&connection).await })
    }

    fn check(&self, setup: &ProviderSetup, secret: Option<&str>) -> Result<String, IdeError> {
        let connection = self.connection(setup, secret)?;
        self.wait(async move {
            let messages = [Message {
                role: Role::User,
                parts: vec![Part::Text("Reply with the one word: ready".to_owned())],
            }];
            let request = Request {
                system: "You check that a connection works. Reply with one word.",
                messages: &messages,
                tools: &[],
            };
            let reply = tokio::time::timeout(
                CHECK_TIMEOUT,
                providers::stream(&connection, &request, &mut |_| {}),
            )
            .await
            .map_err(|_| "the model didn't answer in time".to_owned())??;
            let text: String = reply
                .parts
                .iter()
                .filter_map(|part| match part {
                    Part::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            let text = text.trim();
            if text.is_empty() {
                return Err("the model answered with nothing".to_owned());
            }
            Ok(text.chars().take(200).collect())
        })
    }

    fn send(
        &self,
        conversation: &str,
        text: &str,
        context: ChatContext,
        tools: Arc<dyn WorkspaceTools>,
        sink: AssistantSink,
    ) -> Result<(), IdeError> {
        if text.trim().is_empty() {
            return Err(IdeError::InvalidArgument(
                "write a message first".to_owned(),
            ));
        }
        let setup = config::read(&self.dir)?.ok_or_else(|| {
            IdeError::InvalidArgument("set the assistant up first: File → AI Assistant".to_owned())
        })?;
        let connection = self.connection(&setup, None)?;
        let chat = {
            let mut chats = self
                .chats
                .lock()
                .map_err(|_| IdeError::Io("the assistant failed".to_owned()))?;
            Arc::clone(chats.entry(conversation.to_owned()).or_default())
        };
        let cancel = {
            let mut running = chat
                .cancel
                .lock()
                .map_err(|_| IdeError::Io("the assistant failed".to_owned()))?;
            if running.is_some() {
                return Err(IdeError::InvalidArgument(
                    "a reply is still coming: stop it first".to_owned(),
                ));
            }
            let (stop, cancel) = watch::channel(false);
            *running = Some(stop);
            cancel
        };
        let turn = Turn {
            conversation: conversation.to_owned(),
            connection,
            chat,
            context,
            tools,
            sink,
            cancel,
        };
        self.engine()?
            .handle
            .spawn(conversation::run(turn, text.to_owned()));
        Ok(())
    }

    fn stop(&self, conversation: &str) {
        if let Some(chat) = self.chat(conversation)
            && let Ok(cancel) = chat.cancel.lock()
            && let Some(cancel) = cancel.as_ref()
        {
            let _ = cancel.send(true);
        }
    }

    fn decide(&self, conversation: &str, call_id: &str, accept: bool) -> Result<(), IdeError> {
        let waiting = self
            .chat(conversation)
            .and_then(|chat| chat.pending.lock().ok()?.remove(call_id));
        match waiting {
            Some(decided) => {
                let _ = decided.send(accept);
                Ok(())
            }
            None => Err(IdeError::InvalidArgument(
                "that change isn't waiting any more".to_owned(),
            )),
        }
    }

    fn clear(&self, conversation: &str) {
        self.stop(conversation);
        if let Ok(mut chats) = self.chats.lock() {
            chats.remove(conversation);
        }
    }
}
