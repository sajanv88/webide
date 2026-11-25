//! Amazon Bedrock's Converse API, streamed (ConverseStream), signed with the
//! AWS credentials on this machine (SigV4) or authorized by a Bedrock API key.

use std::time::SystemTime;

use aws_config::Region;
use aws_config::default_provider::credentials::DefaultCredentialsChain;
use aws_credential_types::provider::ProvideCredentials;
use aws_sigv4::http_request::{SignableBody, SignableRequest, SigningSettings, sign};
use aws_sigv4::sign::v4;
use ide_core::assistant::{BedrockAuth, ModelInfo, ProviderKind};
use reqwest::Method;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderName, HeaderValue};
use serde_json::{Value, json};

use super::eventstream::Decoder;
use super::{
    Connection, MAX_TOKENS, Message, Part, Reply, Request, Role, checked, next_chunk,
    parse_arguments, read_json, secret,
};

/// The region when the setup names none.
pub(crate) const DEFAULT_REGION: &str = "us-east-1";

fn region(connection: &Connection) -> String {
    connection
        .setup
        .region
        .as_deref()
        .map(str::trim)
        .filter(|region| !region.is_empty())
        .unwrap_or(DEFAULT_REGION)
        .to_owned()
}

/// A path segment, percent-encoded: model ids hold `:`, and ARNs `/` too.
pub(crate) fn encode_segment(segment: &str) -> String {
    let mut out = String::new();
    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The request's body; `max_tokens` is left out for models that refuse it.
pub(crate) fn body(request: &Request<'_>, max_tokens: bool) -> Value {
    let mut body = json!({
        "system": [{ "text": request.system }],
        "messages": request.messages.iter().map(message).collect::<Vec<_>>(),
    });
    if max_tokens {
        body["inferenceConfig"] = json!({ "maxTokens": MAX_TOKENS });
    }
    if !request.tools.is_empty() {
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|tool| {
                json!({ "toolSpec": {
                    "name": tool.name,
                    "description": tool.description,
                    "inputSchema": { "json": tool.schema },
                }})
            })
            .collect();
        body["toolConfig"] = json!({ "tools": tools });
    }
    body
}

fn message(message: &Message) -> Value {
    let content: Vec<Value> = message
        .parts
        .iter()
        .filter_map(|part| match part {
            // Bedrock refuses empty text.
            Part::Text(text) if text.is_empty() => None,
            Part::Text(text) => Some(json!({ "text": text })),
            Part::ToolUse {
                id, name, input, ..
            } => Some(json!({ "toolUse": { "toolUseId": id, "name": name, "input": input } })),
            Part::ToolResult {
                id,
                output,
                is_error,
                ..
            } => Some(json!({ "toolResult": {
                "toolUseId": id,
                "content": [{ "text": output }],
                "status": if *is_error { "error" } else { "success" },
            }})),
        })
        .collect();
    let role = match message.role {
        Role::User => "user",
        Role::Assistant => "assistant",
    };
    json!({ "role": role, "content": content })
}

enum Block {
    Text(String),
    Tool {
        id: String,
        name: String,
        input: String,
    },
}

/// Reads the stream's events into the reply.
#[derive(Default)]
pub(crate) struct Parser {
    blocks: Vec<(u64, Block)>,
}

impl Parser {
    pub(crate) fn event(
        &mut self,
        message_type: &str,
        event_type: &str,
        payload: &[u8],
        on_text: &mut (dyn FnMut(&str) + Send),
    ) -> Result<(), String> {
        let event: Value = serde_json::from_slice(payload).unwrap_or(Value::Null);
        if message_type == "exception" {
            let said = event["message"]
                .as_str()
                .or(event["Message"].as_str())
                .unwrap_or("an error, without a message");
            return Err(format!("Bedrock stopped the reply ({event_type}): {said}"));
        }
        let index = event["contentBlockIndex"].as_u64().unwrap_or(0);
        match event_type {
            "contentBlockStart" => {
                if let Some(tool) = event.pointer("/start/toolUse") {
                    self.blocks.push((
                        index,
                        Block::Tool {
                            id: tool["toolUseId"].as_str().unwrap_or_default().to_owned(),
                            name: tool["name"].as_str().unwrap_or_default().to_owned(),
                            input: String::new(),
                        },
                    ));
                }
            }
            "contentBlockDelta" => {
                let delta = &event["delta"];
                if let Some(text) = delta["text"].as_str() {
                    on_text(text);
                    match self.blocks.iter_mut().find(|(at, _)| *at == index) {
                        Some((_, Block::Text(previous))) => previous.push_str(text),
                        Some(_) => {}
                        None => self.blocks.push((index, Block::Text(text.to_owned()))),
                    }
                } else if let Some(input) = delta.pointer("/toolUse/input").and_then(Value::as_str)
                    && let Some((_, Block::Tool { input: so_far, .. })) =
                        self.blocks.iter_mut().find(|(at, _)| *at == index)
                {
                    so_far.push_str(input);
                }
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
                Block::Text(text) if text.is_empty() => None,
                Block::Text(text) => Some(Part::Text(text)),
                Block::Tool { id, name, input } => Some(Part::ToolUse {
                    id,
                    name,
                    input: parse_arguments(&input),
                    signature: None,
                }),
            })
            .collect();
        Reply { parts }
    }
}

/// `request` signed (or with the API key), ready to send.
async fn authorize(
    connection: &Connection,
    service: &str,
    mut request: reqwest::Request,
) -> Result<reqwest::Request, String> {
    if connection.setup.bedrock_auth == BedrockAuth::ApiKey {
        let value = HeaderValue::from_str(&format!("Bearer {}", secret(connection)?))
            .map_err(|_| "the Bedrock API key has characters a header can't hold".to_owned())?;
        request.headers_mut().insert(AUTHORIZATION, value);
        return Ok(request);
    }
    let region = region(connection);
    let mut chain = DefaultCredentialsChain::builder().region(Region::new(region.clone()));
    if let Some(profile) = connection
        .setup
        .profile
        .as_deref()
        .filter(|profile| !profile.trim().is_empty())
    {
        chain = chain.profile_name(profile.trim());
    }
    let credentials = chain
        .build()
        .await
        .provide_credentials()
        .await
        .map_err(|error| {
            format!(
                "no AWS credentials were found ({error}). Sign in with the AWS CLI (aws sso login, \
             or aws configure) for the profile in the setup."
            )
        })?;
    let identity = credentials.into();
    let params = v4::SigningParams::builder()
        .identity(&identity)
        .region(&region)
        .name(service)
        .time(SystemTime::now())
        .settings(SigningSettings::default())
        .build()
        .map_err(|error| format!("the request couldn't be signed: {error}"))?
        .into();
    let body = request
        .body()
        .and_then(reqwest::Body::as_bytes)
        .unwrap_or_default()
        .to_vec();
    let url = request.url().to_string();
    let headers: Vec<(String, String)> = request
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            Some((name.as_str().to_owned(), value.to_str().ok()?.to_owned()))
        })
        .collect();
    let signable = SignableRequest::new(
        request.method().as_str(),
        &url,
        headers
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str())),
        SignableBody::Bytes(&body),
    )
    .map_err(|error| format!("the request couldn't be signed: {error}"))?;
    let (instructions, _) = sign(signable, &params)
        .map_err(|error| format!("the request couldn't be signed: {error}"))?
        .into_parts();
    for (name, value) in instructions.headers() {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| "the signature couldn't be added".to_owned())?;
        let value = HeaderValue::from_str(value)
            .map_err(|_| "the signature couldn't be added".to_owned())?;
        request.headers_mut().insert(name, value);
    }
    Ok(request)
}

async fn send(
    connection: &Connection,
    service: &str,
    method: Method,
    url: String,
    body: Option<&Value>,
) -> Result<reqwest::Response, String> {
    let mut builder = connection.http.request(method, url);
    if let Some(body) = body {
        builder = builder
            .header(CONTENT_TYPE, "application/json")
            .body(body.to_string());
    }
    let request = builder
        .build()
        .map_err(|error| format!("the request couldn't be made: {error}"))?;
    let request = authorize(connection, service, request).await?;
    checked(
        ProviderKind::Bedrock,
        connection.http.execute(request).await,
    )
    .await
}

pub(crate) async fn stream(
    connection: &Connection,
    request: &Request<'_>,
    on_text: &mut (dyn FnMut(&str) + Send),
) -> Result<Reply, String> {
    match stream_once(connection, request, on_text, true).await {
        // A model whose limit is lower than ours: let it use its own.
        Err(error)
            if error.contains("maxTokens")
                || error.contains("max_tokens")
                || error.contains("maximum tokens") =>
        {
            stream_once(connection, request, on_text, false).await
        }
        other => other,
    }
}

async fn stream_once(
    connection: &Connection,
    request: &Request<'_>,
    on_text: &mut (dyn FnMut(&str) + Send),
    max_tokens: bool,
) -> Result<Reply, String> {
    let url = format!(
        "https://bedrock-runtime.{}.amazonaws.com/model/{}/converse-stream",
        region(connection),
        encode_segment(connection.setup.model.trim())
    );
    let mut response = send(
        connection,
        "bedrock",
        Method::POST,
        url,
        Some(&body(request, max_tokens)),
    )
    .await?;
    let mut decoder = Decoder::default();
    let mut parser = Parser::default();
    while let Some(chunk) = next_chunk(ProviderKind::Bedrock, &mut response).await? {
        for frame in decoder.feed(&chunk)? {
            parser.event(
                &frame.message_type,
                &frame.event_type,
                &frame.payload,
                on_text,
            )?;
        }
    }
    Ok(parser.finish())
}

/// The text models that answer on demand, and the cross-region inference
/// profiles (which newer models need).
pub(crate) async fn models(connection: &Connection) -> Result<Vec<ModelInfo>, String> {
    let base = format!("https://bedrock.{}.amazonaws.com", region(connection));
    let response = send(
        connection,
        "bedrock",
        Method::GET,
        format!("{base}/foundation-models?byInferenceType=ON_DEMAND&byOutputModality=TEXT"),
        None,
    )
    .await?;
    let json = read_json(ProviderKind::Bedrock, response).await?;
    let mut models: Vec<ModelInfo> = json["modelSummaries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|model| {
            Some(ModelInfo {
                id: model["modelId"].as_str()?.to_owned(),
                name: model["modelName"].as_str().map(str::to_owned),
            })
        })
        .collect();
    // Profiles are a bonus: an account may not be allowed to list them.
    if let Ok(response) = send(
        connection,
        "bedrock",
        Method::GET,
        format!("{base}/inference-profiles?maxResults=1000&type=SYSTEM_DEFINED"),
        None,
    )
    .await
        && let Ok(json) = read_json(ProviderKind::Bedrock, response).await
    {
        models.extend(
            json["inferenceProfileSummaries"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|profile| {
                    Some(ModelInfo {
                        id: profile["inferenceProfileId"].as_str()?.to_owned(),
                        name: profile["inferenceProfileName"].as_str().map(str::to_owned),
                    })
                }),
        );
    }
    Ok(models)
}
