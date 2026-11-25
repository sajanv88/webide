//! Anthropic's Messages API, streamed: https://docs.anthropic.com/en/api/messages

use ide_core::assistant::{ModelInfo, ProviderKind};
use serde_json::{Value, json};

use super::sse::Reader;
use super::{
    Connection, MAX_TOKENS, Message, Part, Reply, Request, Role, checked, next_chunk,
    parse_arguments, read_json, secret, with_json,
};

const BASE: &str = "https://api.anthropic.com/v1";
const VERSION: &str = "2023-06-01";

/// The request's body.
pub(crate) fn body(model: &str, request: &Request<'_>) -> Value {
    let mut body = json!({
        "model": model,
        "max_tokens": MAX_TOKENS,
        "stream": true,
        "system": request.system,
        "messages": request.messages.iter().map(message).collect::<Vec<_>>(),
    });
    if !request.tools.is_empty() {
        body["tools"] = request
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "input_schema": tool.schema,
                })
            })
            .collect();
    }
    body
}

fn message(message: &Message) -> Value {
    let content: Vec<Value> = message
        .parts
        .iter()
        .map(|part| match part {
            Part::Text(text) => json!({ "type": "text", "text": text }),
            Part::ToolUse {
                id, name, input, ..
            } => json!({ "type": "tool_use", "id": id, "name": name, "input": input }),
            Part::ToolResult {
                id,
                output,
                is_error,
                ..
            } => json!({
                "type": "tool_result",
                "tool_use_id": id,
                "content": output,
                "is_error": is_error,
            }),
        })
        .collect();
    let role = match message.role {
        Role::User => "user",
        Role::Assistant => "assistant",
    };
    json!({ "role": role, "content": content })
}

/// A block of the reply, as it arrives.
enum Block {
    Text(String),
    Tool {
        id: String,
        name: String,
        input: String,
    },
    Other,
}

/// Reads the stream's events into the reply.
#[derive(Default)]
pub(crate) struct Parser {
    blocks: Vec<(usize, Block)>,
}

impl Parser {
    pub(crate) fn event(
        &mut self,
        name: &str,
        data: &str,
        on_text: &mut (dyn FnMut(&str) + Send),
    ) -> Result<(), String> {
        let Ok(event) = serde_json::from_str::<Value>(data) else {
            return Ok(());
        };
        let index = event["index"].as_u64().unwrap_or(0) as usize;
        match name {
            "content_block_start" => {
                let block = &event["content_block"];
                let started = match block["type"].as_str() {
                    Some("text") => {
                        let text = block["text"].as_str().unwrap_or_default();
                        if !text.is_empty() {
                            on_text(text);
                        }
                        Block::Text(text.to_owned())
                    }
                    Some("tool_use") => Block::Tool {
                        id: block["id"].as_str().unwrap_or_default().to_owned(),
                        name: block["name"].as_str().unwrap_or_default().to_owned(),
                        input: String::new(),
                    },
                    _ => Block::Other,
                };
                self.blocks.push((index, started));
            }
            "content_block_delta" => {
                let delta = &event["delta"];
                let Some((_, block)) = self.blocks.iter_mut().find(|(at, _)| *at == index) else {
                    return Ok(());
                };
                match (delta["type"].as_str(), block) {
                    (Some("text_delta"), Block::Text(text)) => {
                        let piece = delta["text"].as_str().unwrap_or_default();
                        on_text(piece);
                        text.push_str(piece);
                    }
                    (Some("input_json_delta"), Block::Tool { input, .. }) => {
                        input.push_str(delta["partial_json"].as_str().unwrap_or_default());
                    }
                    _ => {}
                }
            }
            "error" => {
                let said = event
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("an error, without a message");
                return Err(format!("Anthropic stopped the reply: {said}"));
            }
            _ => {}
        }
        Ok(())
    }

    pub(crate) fn finish(self) -> Reply {
        let parts = self
            .blocks
            .into_iter()
            .filter_map(|(_, block)| match block {
                Block::Text(text) if !text.is_empty() => Some(Part::Text(text)),
                Block::Tool { id, name, input } => Some(Part::ToolUse {
                    id,
                    name,
                    input: parse_arguments(&input),
                    signature: None,
                }),
                _ => None,
            })
            .collect();
        Reply { parts }
    }
}

pub(crate) async fn stream(
    connection: &Connection,
    request: &Request<'_>,
    on_text: &mut (dyn FnMut(&str) + Send),
) -> Result<Reply, String> {
    let key = secret(connection)?;
    let builder = connection
        .http
        .post(format!("{BASE}/messages"))
        .header("x-api-key", key)
        .header("anthropic-version", VERSION);
    let response = with_json(builder, &body(&connection.setup.model, request))
        .send()
        .await;
    let mut response = checked(ProviderKind::Anthropic, response).await?;
    let mut reader = Reader::default();
    let mut parser = Parser::default();
    while let Some(chunk) = next_chunk(ProviderKind::Anthropic, &mut response).await? {
        for event in reader.feed(&chunk) {
            parser.event(&event.name, &event.data, on_text)?;
        }
    }
    Ok(parser.finish())
}

pub(crate) async fn models(connection: &Connection) -> Result<Vec<ModelInfo>, String> {
    let key = secret(connection)?;
    let response = connection
        .http
        .get(format!("{BASE}/models?limit=1000"))
        .header("x-api-key", key)
        .header("anthropic-version", VERSION)
        .send()
        .await;
    let response = checked(ProviderKind::Anthropic, response).await?;
    let json = read_json(ProviderKind::Anthropic, response).await?;
    Ok(json["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|model| {
            Some(ModelInfo {
                id: model["id"].as_str()?.to_owned(),
                name: model["display_name"].as_str().map(str::to_owned),
            })
        })
        .collect())
}
