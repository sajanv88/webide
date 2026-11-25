//! OpenAI's Chat Completions API, streamed. Azure AI Foundry (its v1 API)
//! and Ollama speak it too, at their own addresses.

use ide_core::assistant::{ModelInfo, ProviderKind};
use reqwest::RequestBuilder;
use serde_json::{Value, json};

use super::sse::Reader;
use super::{
    Connection, Message, Part, Reply, Request, Role, checked, next_chunk, parse_arguments,
    read_json, secret, with_json,
};

const OPENAI: &str = "https://api.openai.com/v1";
/// Where Ollama listens unless the person says otherwise.
pub(crate) const OLLAMA: &str = "http://127.0.0.1:11434";

/// Ollama's server, without a slash at the end.
pub(crate) fn ollama_root(endpoint: Option<&str>) -> String {
    endpoint
        .map(str::trim)
        .filter(|endpoint| !endpoint.is_empty())
        .unwrap_or(OLLAMA)
        .trim_end_matches('/')
        .trim_end_matches("/v1")
        .to_owned()
}

/// An Azure resource's endpoint as the v1 API's base: `…/openai/v1`, from
/// the endpoint as the portal shows it, with or without that path.
pub(crate) fn azure_base(endpoint: &str) -> Result<String, String> {
    let endpoint = endpoint.trim().trim_end_matches('/');
    if !endpoint.starts_with("https://") {
        return Err("the Azure endpoint must start with https://".to_owned());
    }
    let root = endpoint
        .strip_suffix("/openai/v1")
        .or_else(|| endpoint.strip_suffix("/openai"))
        .unwrap_or(endpoint);
    Ok(format!("{root}/openai/v1"))
}

/// The header that authorizes a request, and its value.
type Auth = Option<(&'static str, String)>;

/// Where requests go, with what authorizes them.
fn base(connection: &Connection) -> Result<(String, Auth), String> {
    match connection.setup.kind {
        ProviderKind::AzureFoundry => {
            let endpoint = connection
                .setup
                .endpoint
                .as_deref()
                .ok_or_else(|| "no Azure endpoint is set up".to_owned())?;
            Ok((
                azure_base(endpoint)?,
                Some(("api-key", secret(connection)?.to_owned())),
            ))
        }
        ProviderKind::Ollama => Ok((
            format!("{}/v1", ollama_root(connection.setup.endpoint.as_deref())),
            None,
        )),
        _ => Ok((
            OPENAI.to_owned(),
            Some(("authorization", format!("Bearer {}", secret(connection)?))),
        )),
    }
}

fn authorized(builder: RequestBuilder, auth: &Auth) -> RequestBuilder {
    match auth {
        Some((header, value)) => builder.header(*header, value),
        None => builder,
    }
}

/// The request's body.
pub(crate) fn body(model: &str, request: &Request<'_>, with_tools: bool) -> Value {
    let mut messages = vec![json!({ "role": "system", "content": request.system })];
    for message in request.messages {
        messages.extend(wire_messages(message));
    }
    let mut body = json!({ "model": model, "stream": true, "messages": messages });
    if with_tools && !request.tools.is_empty() {
        body["tools"] = request
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.schema,
                    },
                })
            })
            .collect();
    }
    body
}

/// A message as the API takes it: a tool's results are messages of their own.
fn wire_messages(message: &Message) -> Vec<Value> {
    let text: String = message
        .parts
        .iter()
        .filter_map(|part| match part {
            Part::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    match message.role {
        Role::User => {
            let mut out: Vec<Value> = message
                .parts
                .iter()
                .filter_map(|part| match part {
                    Part::ToolResult { id, output, .. } => Some(json!({
                        "role": "tool",
                        "tool_call_id": id,
                        "content": output,
                    })),
                    _ => None,
                })
                .collect();
            if !text.is_empty() {
                out.push(json!({ "role": "user", "content": text }));
            }
            out
        }
        Role::Assistant => {
            let calls: Vec<Value> = message
                .parts
                .iter()
                .filter_map(|part| match part {
                    Part::ToolUse {
                        id, name, input, ..
                    } => Some(json!({
                        "id": id,
                        "type": "function",
                        "function": { "name": name, "arguments": input.to_string() },
                    })),
                    _ => None,
                })
                .collect();
            let mut out = json!({
                "role": "assistant",
                "content": if text.is_empty() { Value::Null } else { Value::String(text) },
            });
            if !calls.is_empty() {
                out["tool_calls"] = Value::Array(calls);
            }
            vec![out]
        }
    }
}

/// A tool call as it arrives, a piece at a time.
#[derive(Default)]
struct Call {
    id: String,
    name: String,
    arguments: String,
}

/// Reads the stream's chunks into the reply.
#[derive(Default)]
pub(crate) struct Parser {
    text: String,
    calls: Vec<(u64, Call)>,
}

impl Parser {
    /// One `data:` line; `false` once the stream says it's done.
    pub(crate) fn data(
        &mut self,
        data: &str,
        on_text: &mut (dyn FnMut(&str) + Send),
    ) -> Result<bool, String> {
        if data.trim() == "[DONE]" {
            return Ok(false);
        }
        let Ok(chunk) = serde_json::from_str::<Value>(data) else {
            return Ok(true);
        };
        if let Some(said) = chunk.pointer("/error/message").and_then(Value::as_str) {
            return Err(format!("the model stopped the reply: {said}"));
        }
        let Some(delta) = chunk.pointer("/choices/0/delta") else {
            return Ok(true);
        };
        if let Some(text) = delta["content"].as_str()
            && !text.is_empty()
        {
            on_text(text);
            self.text.push_str(text);
        }
        for piece in delta["tool_calls"].as_array().into_iter().flatten() {
            let index = piece["index"].as_u64().unwrap_or(0);
            let at = match self.calls.iter().position(|(at, _)| *at == index) {
                Some(at) => at,
                None => {
                    self.calls.push((index, Call::default()));
                    self.calls.len() - 1
                }
            };
            let call = &mut self.calls[at].1;
            if let Some(id) = piece["id"].as_str() {
                id.clone_into(&mut call.id);
            }
            if let Some(name) = piece.pointer("/function/name").and_then(Value::as_str) {
                call.name.push_str(name);
            }
            if let Some(arguments) = piece.pointer("/function/arguments").and_then(Value::as_str) {
                call.arguments.push_str(arguments);
            }
        }
        Ok(true)
    }

    pub(crate) fn finish(self) -> Reply {
        let mut parts = Vec::new();
        if !self.text.is_empty() {
            parts.push(Part::Text(self.text));
        }
        for (index, call) in self.calls {
            parts.push(Part::ToolUse {
                id: if call.id.is_empty() {
                    format!("call_{index}")
                } else {
                    call.id
                },
                name: call.name,
                input: parse_arguments(&call.arguments),
                signature: None,
            });
        }
        Reply { parts }
    }
}

pub(crate) async fn stream(
    connection: &Connection,
    request: &Request<'_>,
    on_text: &mut (dyn FnMut(&str) + Send),
) -> Result<Reply, String> {
    let kind = connection.setup.kind;
    match stream_once(connection, request, on_text, true).await {
        // Some of Ollama's models can't use tools: answer without them.
        Err(error) if kind == ProviderKind::Ollama && error.contains("does not support tools") => {
            on_text("_This model can't use tools, so it can't read the folder._\n\n");
            stream_once(connection, request, on_text, false).await
        }
        other => other,
    }
}

async fn stream_once(
    connection: &Connection,
    request: &Request<'_>,
    on_text: &mut (dyn FnMut(&str) + Send),
    with_tools: bool,
) -> Result<Reply, String> {
    let kind = connection.setup.kind;
    let (base, auth) = base(connection)?;
    let builder = authorized(
        connection.http.post(format!("{base}/chat/completions")),
        &auth,
    );
    let response = with_json(builder, &body(&connection.setup.model, request, with_tools))
        .send()
        .await;
    let mut response = checked(kind, response).await?;
    let mut reader = Reader::default();
    let mut parser = Parser::default();
    'stream: while let Some(chunk) = next_chunk(kind, &mut response).await? {
        for event in reader.feed(&chunk) {
            if !parser.data(&event.data, on_text)? {
                break 'stream;
            }
        }
    }
    Ok(parser.finish())
}

/// Words in the ids of OpenAI's models that don't chat.
const NOT_CHAT: [&str; 12] = [
    "embedding",
    "tts",
    "whisper",
    "dall-e",
    "moderation",
    "image",
    "audio",
    "realtime",
    "transcribe",
    "search",
    "babbage",
    "davinci",
];

pub(crate) async fn models(connection: &Connection) -> Result<Vec<ModelInfo>, String> {
    let kind = connection.setup.kind;
    if kind == ProviderKind::Ollama {
        let root = ollama_root(connection.setup.endpoint.as_deref());
        let response = connection.http.get(format!("{root}/api/tags")).send().await;
        let json = read_json(kind, checked(kind, response).await?).await?;
        return Ok(json["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|model| {
                Some(ModelInfo {
                    id: model["name"]
                        .as_str()
                        .or(model["model"].as_str())?
                        .to_owned(),
                    name: None,
                })
            })
            .collect());
    }
    let (base, auth) = base(connection)?;
    let response = authorized(connection.http.get(format!("{base}/models")), &auth)
        .send()
        .await;
    let json = read_json(kind, checked(kind, response).await?).await?;
    Ok(json["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|model| model["id"].as_str())
        .filter(|id| !NOT_CHAT.iter().any(|word| id.contains(word)))
        .map(|id| ModelInfo {
            id: id.to_owned(),
            name: None,
        })
        .collect())
}
