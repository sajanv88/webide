//! The Gemini API's generateContent, streamed:
//! https://ai.google.dev/api/generate-content

use ide_core::assistant::{ModelInfo, ProviderKind};
use serde_json::{Value, json};

use super::sse::Reader;
use super::{
    Connection, Message, Part, Reply, Request, Role, checked, next_chunk, read_json, secret,
    with_json,
};

const BASE: &str = "https://generativelanguage.googleapis.com/v1beta";
/// Ids made here for calls Gemini gave none; they aren't sent back.
const MADE_ID: &str = "gemini-call-";

/// The request's body.
pub(crate) fn body(request: &Request<'_>) -> Value {
    let mut body = json!({
        "systemInstruction": { "parts": [{ "text": request.system }] },
        "contents": request.messages.iter().map(content).collect::<Vec<_>>(),
    });
    if !request.tools.is_empty() {
        let declarations: Vec<Value> = request
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "parametersJsonSchema": tool.schema,
                })
            })
            .collect();
        body["tools"] = json!([{ "functionDeclarations": declarations }]);
    }
    body
}

fn content(message: &Message) -> Value {
    let parts: Vec<Value> = message
        .parts
        .iter()
        .map(|part| match part {
            Part::Text(text) => json!({ "text": text }),
            Part::ToolUse {
                id,
                name,
                input,
                signature,
            } => {
                let mut call = json!({ "name": name, "args": input });
                if !id.starts_with(MADE_ID) {
                    call["id"] = json!(id);
                }
                let mut part = json!({ "functionCall": call });
                if let Some(signature) = signature {
                    part["thoughtSignature"] = json!(signature);
                }
                part
            }
            Part::ToolResult {
                id,
                name,
                output,
                is_error,
            } => {
                let response = if *is_error {
                    json!({ "error": output })
                } else {
                    json!({ "output": output })
                };
                let mut answer = json!({ "name": name, "response": response });
                if !id.starts_with(MADE_ID) {
                    answer["id"] = json!(id);
                }
                json!({ "functionResponse": answer })
            }
        })
        .collect();
    let role = match message.role {
        Role::User => "user",
        Role::Assistant => "model",
    };
    json!({ "role": role, "parts": parts })
}

/// Reads the stream's chunks into the reply.
#[derive(Default)]
pub(crate) struct Parser {
    parts: Vec<Part>,
    calls: usize,
}

impl Parser {
    pub(crate) fn data(
        &mut self,
        data: &str,
        on_text: &mut (dyn FnMut(&str) + Send),
    ) -> Result<(), String> {
        let Ok(chunk) = serde_json::from_str::<Value>(data) else {
            return Ok(());
        };
        if let Some(said) = chunk.pointer("/error/message").and_then(Value::as_str) {
            return Err(format!("Gemini stopped the reply: {said}"));
        }
        if let Some(reason) = chunk
            .pointer("/promptFeedback/blockReason")
            .and_then(Value::as_str)
        {
            return Err(format!("Gemini wouldn't answer this ({reason})"));
        }
        let parts = chunk
            .pointer("/candidates/0/content/parts")
            .and_then(Value::as_array);
        for part in parts.into_iter().flatten() {
            // Thought summaries, when a model sends them, aren't the reply.
            if part["thought"].as_bool() == Some(true) {
                continue;
            }
            if let Some(text) = part["text"].as_str() {
                if text.is_empty() {
                    continue;
                }
                on_text(text);
                match self.parts.last_mut() {
                    Some(Part::Text(previous)) => previous.push_str(text),
                    _ => self.parts.push(Part::Text(text.to_owned())),
                }
            } else if let Some(call) = part.get("functionCall") {
                self.calls += 1;
                self.parts.push(Part::ToolUse {
                    id: call["id"]
                        .as_str()
                        .map_or_else(|| format!("{MADE_ID}{}", self.calls), str::to_owned),
                    name: call["name"].as_str().unwrap_or_default().to_owned(),
                    input: call.get("args").cloned().unwrap_or_else(|| json!({})),
                    signature: part["thoughtSignature"].as_str().map(str::to_owned),
                });
            }
        }
        Ok(())
    }

    pub(crate) fn finish(self) -> Reply {
        Reply { parts: self.parts }
    }
}

pub(crate) async fn stream(
    connection: &Connection,
    request: &Request<'_>,
    on_text: &mut (dyn FnMut(&str) + Send),
) -> Result<Reply, String> {
    let key = secret(connection)?;
    let model = connection.setup.model.trim_start_matches("models/");
    let builder = connection
        .http
        .post(format!(
            "{BASE}/models/{model}:streamGenerateContent?alt=sse"
        ))
        .header("x-goog-api-key", key);
    let response = with_json(builder, &body(request)).send().await;
    let mut response = checked(ProviderKind::Gemini, response).await?;
    let mut reader = Reader::default();
    let mut parser = Parser::default();
    while let Some(chunk) = next_chunk(ProviderKind::Gemini, &mut response).await? {
        for event in reader.feed(&chunk) {
            parser.data(&event.data, on_text)?;
        }
    }
    if let Some(event) = reader.finish() {
        parser.data(&event.data, on_text)?;
    }
    Ok(parser.finish())
}

pub(crate) async fn models(connection: &Connection) -> Result<Vec<ModelInfo>, String> {
    let key = secret(connection)?;
    let response = connection
        .http
        .get(format!("{BASE}/models?pageSize=1000"))
        .header("x-goog-api-key", key)
        .send()
        .await;
    let json = read_json(
        ProviderKind::Gemini,
        checked(ProviderKind::Gemini, response).await?,
    )
    .await?;
    Ok(json["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|model| {
            model["supportedGenerationMethods"]
                .as_array()
                .is_some_and(|methods| methods.iter().any(|m| m == "generateContent"))
        })
        .filter_map(|model| {
            Some(ModelInfo {
                id: model["name"]
                    .as_str()?
                    .trim_start_matches("models/")
                    .to_owned(),
                name: model["displayName"].as_str().map(str::to_owned),
            })
        })
        .collect())
}
