//! The providers, each spoken in its own API: a conversation streamed with
//! tools, and the models on offer. What they share is here.

pub(crate) mod anthropic;
pub(crate) mod bedrock;
pub(crate) mod eventstream;
pub(crate) mod gemini;
pub(crate) mod openai;
pub(crate) mod sse;

use ide_core::assistant::{ModelInfo, ProviderKind, ProviderSetup};
use reqwest::header::CONTENT_TYPE;
use reqwest::{RequestBuilder, Response};
use serde_json::Value;

/// Who said it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    User,
    Assistant,
}

/// A piece of a message.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Part {
    Text(String),
    /// The model calls a tool.
    ToolUse {
        id: String,
        name: String,
        input: Value,
        /// Gemini's thought signature, which has to be sent back as it came.
        signature: Option<String>,
    },
    /// What a tool answered.
    ToolResult {
        id: String,
        name: String,
        output: String,
        is_error: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Message {
    pub role: Role,
    pub parts: Vec<Part>,
}

/// A tool as the model is told about it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ToolDef {
    pub name: String,
    pub description: String,
    /// The JSON Schema of its arguments.
    pub schema: Value,
}

pub(crate) struct Request<'a> {
    pub system: &'a str,
    pub messages: &'a [Message],
    pub tools: &'a [ToolDef],
}

/// The model's turn: text, and the tools it calls, in order.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Reply {
    pub parts: Vec<Part>,
}

/// A provider, set up, with what signs or authorizes its requests.
pub(crate) struct Connection {
    pub setup: ProviderSetup,
    pub secret: Option<String>,
    pub http: reqwest::Client,
}

/// The most tokens a reply may take, where the API asks for a limit.
pub(crate) const MAX_TOKENS: u32 = 8192;

/// Streams the model's turn, giving its text to `on_text` as it arrives.
/// Dropping the future stops the request.
pub(crate) async fn stream(
    connection: &Connection,
    request: &Request<'_>,
    on_text: &mut (dyn FnMut(&str) + Send),
) -> Result<Reply, String> {
    match connection.setup.kind {
        ProviderKind::Anthropic => anthropic::stream(connection, request, on_text).await,
        ProviderKind::OpenAi | ProviderKind::AzureFoundry | ProviderKind::Ollama => {
            openai::stream(connection, request, on_text).await
        }
        ProviderKind::Gemini => gemini::stream(connection, request, on_text).await,
        ProviderKind::Bedrock => bedrock::stream(connection, request, on_text).await,
    }
}

/// The models the provider offers, sorted by id. Azure's deployments can't
/// be listed with a key: the person types the deployment's name.
pub(crate) async fn list_models(connection: &Connection) -> Result<Vec<ModelInfo>, String> {
    let mut models = match connection.setup.kind {
        ProviderKind::Anthropic => anthropic::models(connection).await?,
        ProviderKind::OpenAi | ProviderKind::Ollama => openai::models(connection).await?,
        ProviderKind::AzureFoundry => Vec::new(),
        ProviderKind::Gemini => gemini::models(connection).await?,
        ProviderKind::Bedrock => bedrock::models(connection).await?,
    };
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
    Ok(models)
}

/// The provider's name, for messages.
pub(crate) fn provider_name(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Anthropic => "Anthropic",
        ProviderKind::OpenAi => "OpenAI",
        ProviderKind::AzureFoundry => "Azure AI Foundry",
        ProviderKind::Gemini => "Gemini",
        ProviderKind::Ollama => "Ollama",
        ProviderKind::Bedrock => "Amazon Bedrock",
    }
}

/// The secret, or why there's none.
pub(crate) fn secret(connection: &Connection) -> Result<&str, String> {
    connection
        .secret
        .as_deref()
        .filter(|secret| !secret.is_empty())
        .ok_or_else(|| "no API key is saved: set the assistant up again".to_owned())
}

/// `response` once it's known to be a success, or a sentence saying why not,
/// with the provider's own words when it gave any.
pub(crate) async fn checked(
    kind: ProviderKind,
    response: Result<Response, reqwest::Error>,
) -> Result<Response, String> {
    let name = provider_name(kind);
    let response = response.map_err(|error| transport_error(name, &error))?;
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    let said = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|json| error_message(&json))
        .unwrap_or_else(|| body.chars().take(300).collect());
    let hint = match status.as_u16() {
        401 | 403 => " Check the key, or the account's access to this model.",
        404 => " Check the model's name (or the deployment's, on Azure).",
        429 => " The provider is limiting requests: wait a little.",
        _ => "",
    };
    Err(format!(
        "{name} refused the request (HTTP {}): {}{hint}",
        status.as_u16(),
        said.trim()
    ))
}

fn transport_error(name: &str, error: &reqwest::Error) -> String {
    if error.is_connect() {
        format!("couldn't reach {name}: check the address and the network")
    } else if error.is_timeout() {
        format!("{name} didn't answer in time")
    } else {
        format!("the request to {name} failed: {error}")
    }
}

/// The message in a provider's error body, wherever it puts it.
fn error_message(json: &Value) -> Option<String> {
    let json = json.as_array().and_then(|all| all.first()).unwrap_or(json);
    let candidates = [
        json.pointer("/error/message"),
        json.get("message"),
        json.get("Message"),
        json.get("error").filter(|error| error.is_string()),
    ];
    candidates
        .into_iter()
        .flatten()
        .find_map(Value::as_str)
        .map(str::to_owned)
}

/// The next piece of the response's body, or `None` at its end.
pub(crate) async fn next_chunk(
    kind: ProviderKind,
    response: &mut Response,
) -> Result<Option<Vec<u8>>, String> {
    response
        .chunk()
        .await
        .map(|chunk| chunk.map(|bytes| bytes.to_vec()))
        .map_err(|error| format!("the connection to {} broke: {error}", provider_name(kind)))
}

/// `builder` with `body` as its JSON.
pub(crate) fn with_json(builder: RequestBuilder, body: &Value) -> RequestBuilder {
    builder
        .header(CONTENT_TYPE, "application/json")
        .body(body.to_string())
}

/// A whole response's body, as JSON.
pub(crate) async fn read_json(kind: ProviderKind, response: Response) -> Result<Value, String> {
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("{}'s answer couldn't be read: {error}", provider_name(kind)))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("{}'s answer isn't JSON: {error}", provider_name(kind)))
}

/// A tool call's arguments, from text that arrived in pieces: an empty
/// object when there were none.
pub(crate) fn parse_arguments(text: &str) -> Value {
    if text.trim().is_empty() {
        return Value::Object(serde_json::Map::new());
    }
    serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_owned()))
}
