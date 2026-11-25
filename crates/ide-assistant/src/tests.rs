use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use ide_core::agent::{ToolSpec, WorkspaceTools};
use ide_core::assistant::{
    AssistantEvent, AssistantHost, BedrockAuth, ChatContext, ProviderKind, ProviderSetup,
    ToolStatus,
};
use serde_json::{Value, json};

use crate::conversation::{add_user, apply_change};
use crate::providers::eventstream::{Decoder, crc32, encode};
use crate::providers::sse::Reader;
use crate::providers::{Message, Part, Request, Role, ToolDef, anthropic, bedrock, gemini, openai};
use crate::secrets::InMemory;
use crate::{AssistantService, config};

fn setup(kind: ProviderKind) -> ProviderSetup {
    ProviderSetup {
        kind,
        model: "a-model".to_owned(),
        endpoint: None,
        region: None,
        profile: None,
        bedrock_auth: BedrockAuth::Profile,
    }
}

/// A conversation that used a tool, for the request bodies.
fn history() -> Vec<Message> {
    vec![
        Message {
            role: Role::User,
            parts: vec![Part::Text("What's in a.ts?".to_owned())],
        },
        Message {
            role: Role::Assistant,
            parts: vec![
                Part::Text("Reading it.".to_owned()),
                Part::ToolUse {
                    id: "call-1".to_owned(),
                    name: "read_file".to_owned(),
                    input: json!({ "path": "a.ts" }),
                    signature: Some("sig".to_owned()),
                },
            ],
        },
        Message {
            role: Role::User,
            parts: vec![
                Part::ToolResult {
                    id: "call-1".to_owned(),
                    name: "read_file".to_owned(),
                    output: "let a = 1;".to_owned(),
                    is_error: false,
                },
                Part::Text("And b?".to_owned()),
            ],
        },
    ]
}

fn tools() -> Vec<ToolDef> {
    vec![ToolDef {
        name: "read_file".to_owned(),
        description: "Reads a file.".to_owned(),
        schema: json!({ "type": "object" }),
    }]
}

fn collect(text: &mut String) -> impl FnMut(&str) + Send + '_ {
    move |delta| text.push_str(delta)
}

#[test]
fn server_sent_events_arrive_in_pieces() {
    let mut reader = Reader::default();
    assert!(reader.feed(b"event: a\ndata: {\"x\"").is_empty());
    let events = reader.feed(b":1}\n\n: a comment\ndata: one\ndata: two\n\n");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].name, "a");
    assert_eq!(events[0].data, "{\"x\":1}");
    assert_eq!(events[1].data, "one\ntwo");
    reader.feed(b"data: last");
    assert_eq!(
        reader.finish().map(|event| event.data),
        Some("last".to_owned())
    );
}

#[test]
fn event_stream_frames_decode_and_damage_is_caught() {
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    let frame = encode("event", "contentBlockDelta", br#"{"delta":{"text":"hi"}}"#);
    let mut decoder = Decoder::default();
    let (first, second) = frame.split_at(7);
    assert!(decoder.feed(first).unwrap().is_empty());
    let frames = decoder.feed(second).unwrap();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].message_type, "event");
    assert_eq!(frames[0].event_type, "contentBlockDelta");
    assert_eq!(frames[0].payload, br#"{"delta":{"text":"hi"}}"#);

    let mut damaged = frame.clone();
    let last = damaged.len() - 6;
    damaged[last] ^= 0xFF;
    assert!(Decoder::default().feed(&damaged).is_err());
}

#[test]
fn anthropic_streams_text_and_tool_calls() {
    let events = [
        (
            "content_block_start",
            r#"{"index":0,"content_block":{"type":"text","text":""}}"#,
        ),
        (
            "content_block_delta",
            r#"{"index":0,"delta":{"type":"text_delta","text":"Let me "}}"#,
        ),
        (
            "content_block_delta",
            r#"{"index":0,"delta":{"type":"text_delta","text":"look."}}"#,
        ),
        (
            "content_block_start",
            r#"{"index":1,"content_block":{"type":"tool_use","id":"t1","name":"read_file"}}"#,
        ),
        (
            "content_block_delta",
            r#"{"index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":"}}"#,
        ),
        (
            "content_block_delta",
            r#"{"index":1,"delta":{"type":"input_json_delta","partial_json":"\"a.ts\"}"}}"#,
        ),
    ];
    let mut text = String::new();
    let mut parser = anthropic::Parser::default();
    for (name, data) in events {
        parser.event(name, data, &mut collect(&mut text)).unwrap();
    }
    assert_eq!(text, "Let me look.");
    assert_eq!(
        parser.finish().parts,
        vec![
            Part::Text("Let me look.".to_owned()),
            Part::ToolUse {
                id: "t1".to_owned(),
                name: "read_file".to_owned(),
                input: json!({ "path": "a.ts" }),
                signature: None,
            },
        ]
    );
    let mut parser = anthropic::Parser::default();
    let error = parser
        .event(
            "error",
            r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
            &mut |_| {},
        )
        .unwrap_err();
    assert!(error.contains("Overloaded"), "{error}");
}

#[test]
fn anthropic_requests_carry_tool_results() {
    let messages = history();
    let body = anthropic::body(
        "claude",
        &Request {
            system: "be brief",
            messages: &messages,
            tools: &tools(),
        },
    );
    assert_eq!(body["system"], "be brief");
    assert_eq!(
        body["tools"][0]["input_schema"],
        json!({ "type": "object" })
    );
    assert_eq!(body["messages"][1]["content"][1]["type"], "tool_use");
    assert_eq!(body["messages"][2]["content"][0]["type"], "tool_result");
    assert_eq!(body["messages"][2]["content"][0]["tool_use_id"], "call-1");
    assert_eq!(body["messages"][2]["content"][1]["text"], "And b?");
}

#[test]
fn openai_streams_text_and_tool_calls_in_pieces() {
    let chunks = [
        r#"{"choices":[]}"#,
        r#"{"choices":[{"delta":{"content":"Hi"}}]}"#,
        r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"read_file","arguments":"{\"pa"}}]}}]}"#,
        r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"th\":\"a.ts\"}"}}]}}]}"#,
    ];
    let mut text = String::new();
    let mut parser = openai::Parser::default();
    for chunk in chunks {
        assert!(parser.data(chunk, &mut collect(&mut text)).unwrap());
    }
    assert!(!parser.data("[DONE]", &mut |_| {}).unwrap());
    assert_eq!(text, "Hi");
    assert_eq!(
        parser.finish().parts,
        vec![
            Part::Text("Hi".to_owned()),
            Part::ToolUse {
                id: "c1".to_owned(),
                name: "read_file".to_owned(),
                input: json!({ "path": "a.ts" }),
                signature: None,
            },
        ]
    );
}

#[test]
fn openai_requests_put_tool_results_before_the_next_text() {
    let messages = history();
    let request = Request {
        system: "be brief",
        messages: &messages,
        tools: &tools(),
    };
    let body = openai::body("gpt", &request, true);
    let wire = body["messages"].as_array().unwrap();
    let roles: Vec<&str> = wire.iter().map(|m| m["role"].as_str().unwrap()).collect();
    assert_eq!(roles, ["system", "user", "assistant", "tool", "user"]);
    assert_eq!(
        wire[2]["tool_calls"][0]["function"]["arguments"],
        r#"{"path":"a.ts"}"#
    );
    assert_eq!(wire[3]["tool_call_id"], "call-1");
    assert_eq!(body["tools"][0]["function"]["name"], "read_file");
    assert!(openai::body("gpt", &request, false).get("tools").is_none());
}

#[test]
fn azure_and_ollama_addresses() {
    for endpoint in [
        "https://r.openai.azure.com",
        "https://r.openai.azure.com/",
        "https://r.openai.azure.com/openai/v1/",
        "https://r.openai.azure.com/openai",
    ] {
        assert_eq!(
            openai::azure_base(endpoint).unwrap(),
            "https://r.openai.azure.com/openai/v1"
        );
    }
    assert!(openai::azure_base("http://r.openai.azure.com").is_err());
    assert_eq!(openai::ollama_root(None), "http://127.0.0.1:11434");
    assert_eq!(
        openai::ollama_root(Some("http://box:11434/v1/")),
        "http://box:11434"
    );
}

#[test]
fn gemini_streams_and_sends_signatures_back() {
    let mut text = String::new();
    let mut parser = gemini::Parser::default();
    for chunk in [
        r#"{"candidates":[{"content":{"parts":[{"text":"Look"}]}}]}"#,
        r#"{"candidates":[{"content":{"parts":[{"text":"ing.","thought":false},{"text":"hidden","thought":true}]}}]}"#,
        r#"{"candidates":[{"content":{"parts":[{"functionCall":{"name":"read_file","args":{"path":"a.ts"}},"thoughtSignature":"s1"}]}}]}"#,
    ] {
        parser.data(chunk, &mut collect(&mut text)).unwrap();
    }
    assert_eq!(text, "Looking.");
    let parts = parser.finish().parts;
    assert_eq!(parts[0], Part::Text("Looking.".to_owned()));
    let Part::ToolUse { id, signature, .. } = &parts[1] else {
        panic!("not a call: {parts:?}");
    };
    assert_eq!(signature.as_deref(), Some("s1"));

    // A made-up id isn't sent back; a signature is.
    let messages = vec![Message {
        role: Role::Assistant,
        parts: parts.clone(),
    }];
    let body = gemini::body(&Request {
        system: "s",
        messages: &messages,
        tools: &tools(),
    });
    let call = &body["contents"][0]["parts"][1];
    assert_eq!(call["thoughtSignature"], "s1");
    assert!(call["functionCall"].get("id").is_none(), "{id}");
    assert_eq!(body["contents"][0]["role"], "model");
    assert_eq!(
        body["tools"][0]["functionDeclarations"][0]["parametersJsonSchema"],
        json!({ "type": "object" })
    );
}

#[test]
fn bedrock_streams_and_encodes_model_ids() {
    let mut text = String::new();
    let mut parser = bedrock::Parser::default();
    let events: [(&str, Value); 4] = [
        (
            "contentBlockDelta",
            json!({ "contentBlockIndex": 0, "delta": { "text": "On it." } }),
        ),
        (
            "contentBlockStart",
            json!({ "contentBlockIndex": 1, "start": { "toolUse": { "toolUseId": "b1", "name": "read_file" } } }),
        ),
        (
            "contentBlockDelta",
            json!({ "contentBlockIndex": 1, "delta": { "toolUse": { "input": "{\"path\":\"a.ts\"}" } } }),
        ),
        ("messageStop", json!({ "stopReason": "tool_use" })),
    ];
    for (name, payload) in events {
        parser
            .event(
                "event",
                name,
                payload.to_string().as_bytes(),
                &mut collect(&mut text),
            )
            .unwrap();
    }
    assert_eq!(text, "On it.");
    assert_eq!(parser.finish().parts.len(), 2);
    let error = bedrock::Parser::default()
        .event(
            "exception",
            "throttlingException",
            br#"{"message":"Too many requests"}"#,
            &mut |_| {},
        )
        .unwrap_err();
    assert!(error.contains("Too many requests"), "{error}");

    assert_eq!(
        bedrock::encode_segment("anthropic.claude-v2:1"),
        "anthropic.claude-v2%3A1"
    );
    let messages = history();
    let body = bedrock::body(
        &Request {
            system: "s",
            messages: &messages,
            tools: &tools(),
        },
        true,
    );
    assert_eq!(
        body["messages"][2]["content"][0]["toolResult"]["status"],
        "success"
    );
    assert_eq!(
        body["toolConfig"]["tools"][0]["toolSpec"]["inputSchema"]["json"],
        json!({ "type": "object" })
    );
    assert_eq!(body["inferenceConfig"]["maxTokens"], 8192);
}

#[test]
fn changes_replace_one_occurrence_or_the_whole_file() {
    assert_eq!(
        apply_change("a", Some("one two"), Some("two"), "2").unwrap(),
        "one 2"
    );
    assert_eq!(
        apply_change("a", Some("x"), None, "all new").unwrap(),
        "all new"
    );
    assert_eq!(
        apply_change("a", None, None, "new file").unwrap(),
        "new file"
    );
    assert!(
        apply_change("a", Some("x x"), Some("x"), "y")
            .unwrap_err()
            .contains("2 times")
    );
    assert!(
        apply_change("a", Some("x"), Some("z"), "y")
            .unwrap_err()
            .contains("isn't in")
    );
    assert!(
        apply_change("a", None, Some("z"), "y")
            .unwrap_err()
            .contains("doesn't exist")
    );
}

#[test]
fn a_message_after_a_failed_reply_joins_the_last_one() {
    let mut messages = Vec::new();
    add_user(&mut messages, vec![Part::Text("one".to_owned())]);
    add_user(&mut messages, vec![Part::Text("two".to_owned())]);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].parts.len(), 2);
}

#[test]
fn setups_are_validated_and_kept_without_keys() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(config::read(dir.path()).unwrap(), None);
    let mut azure = setup(ProviderKind::AzureFoundry);
    assert!(config::validate(&azure).is_err());
    azure.endpoint = Some("https://r.openai.azure.com".to_owned());
    assert!(config::validate(&azure).is_ok());
    let mut bedrock = setup(ProviderKind::Bedrock);
    bedrock.region = Some("US East".to_owned());
    assert!(config::validate(&bedrock).is_err());

    let service =
        AssistantService::with_secrets(dir.path().to_path_buf(), Arc::new(InMemory::default()));
    // A key is needed, and kept out of the file.
    assert!(service.save(&setup(ProviderKind::Anthropic), None).is_err());
    let status = service
        .save(&setup(ProviderKind::Anthropic), Some(" sk-secret "))
        .unwrap();
    assert!(status.has_secret);
    assert_eq!(status.setup.unwrap().kind, ProviderKind::Anthropic);
    let file = std::fs::read_to_string(dir.path().join(config::FILE)).unwrap();
    assert!(!file.contains("sk-secret"), "{file}");
    // Ollama needs none.
    let status = service.save(&setup(ProviderKind::Ollama), None).unwrap();
    assert!(!status.has_secret);
    let status = service.forget().unwrap();
    assert_eq!(status.setup, None);
}

/// Tools that read from a map and remember what's written.
#[derive(Default)]
struct FakeTools {
    files: Mutex<std::collections::HashMap<String, String>>,
    calls: Mutex<Vec<String>>,
}

impl WorkspaceTools for FakeTools {
    fn read_tools(&self) -> Vec<ToolSpec> {
        vec![ToolSpec {
            name: "read_file".to_owned(),
            description: "Reads a file.".to_owned(),
            input_schema: r#"{"type":"object"}"#.to_owned(),
        }]
    }

    fn call(&self, name: &str, arguments: &str) -> Result<String, String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("{name} {arguments}"));
        let args: Value = serde_json::from_str(arguments).map_err(|e| e.to_string())?;
        let path = args["path"].as_str().unwrap_or_default();
        self.files
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or_else(|| "not found".to_owned())
    }

    fn read_text(&self, path: &str) -> Result<(String, Option<String>), String> {
        Ok((
            path.to_owned(),
            self.files.lock().unwrap().get(path).cloned(),
        ))
    }

    fn write_text(&self, path: &str, text: &str) -> Result<(), String> {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_owned(), text.to_owned());
        Ok(())
    }
}

/// An OpenAI-compatible server (as Ollama is) that answers each request
/// with the next of `replies`, as server-sent events; it hands back the
/// bodies it was sent.
fn fake_server(replies: Vec<String>) -> (u16, mpsc::Receiver<Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (sent, bodies) = mpsc::channel();
    thread::spawn(move || {
        for reply in replies {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let _ = sent.send(serde_json::from_slice(&body).unwrap_or(Value::Null));
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
                reply.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
        }
    });
    (port, bodies)
}

fn sse(chunks: &[Value]) -> String {
    let mut out: String = chunks
        .iter()
        .map(|chunk| format!("data: {chunk}\n\n"))
        .collect();
    out.push_str("data: [DONE]\n\n");
    out
}

fn tool_call(id: &str, name: &str, arguments: Value) -> Value {
    json!({ "choices": [{ "delta": { "tool_calls": [{
        "index": 0, "id": id, "function": { "name": name, "arguments": arguments.to_string() }
    }]}}]})
}

#[test]
fn a_conversation_reads_proposes_and_writes_once_accepted() {
    let (port, bodies) = fake_server(vec![
        sse(&[tool_call("c1", "read_file", json!({ "path": "a.ts" }))]),
        sse(&[tool_call(
            "c2",
            "propose_edit",
            json!({ "path": "a.ts", "old_text": "1", "new_text": "2", "summary": "Bump a." }),
        )]),
        sse(&[json!({ "choices": [{ "delta": { "content": "Done: a is 2." } }] })]),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let service =
        AssistantService::with_secrets(dir.path().to_path_buf(), Arc::new(InMemory::default()));
    let mut ollama = setup(ProviderKind::Ollama);
    ollama.endpoint = Some(format!("http://127.0.0.1:{port}"));
    service.save(&ollama, None).unwrap();

    let tools = Arc::new(FakeTools::default());
    tools
        .files
        .lock()
        .unwrap()
        .insert("a.ts".to_owned(), "let a = 1;".to_owned());
    let (events_in, events) = mpsc::channel();
    let events_in = Mutex::new(events_in);
    service
        .send(
            "chat-1",
            "Make a 2",
            ChatContext {
                workspace: Some("demo".to_owned()),
                active_path: Some("a.ts".to_owned()),
            },
            Arc::clone(&tools) as Arc<dyn WorkspaceTools>,
            Arc::new(move |event| {
                let _ = events_in.lock().unwrap().send(event);
            }),
        )
        .unwrap();
    // A second message while the reply comes is refused.
    assert!(
        service
            .send(
                "chat-1",
                "again",
                ChatContext::default(),
                Arc::clone(&tools) as _,
                Arc::new(|_| {})
            )
            .is_err()
    );

    let next = || events.recv_timeout(Duration::from_secs(10)).unwrap();
    let mut seen = Vec::new();
    loop {
        let event = next();
        if let AssistantEvent::Proposal {
            call_id,
            path,
            original,
            proposed,
            summary,
            ..
        } = &event
        {
            assert_eq!(path, "a.ts");
            assert_eq!(original.as_deref(), Some("let a = 1;"));
            assert_eq!(proposed, "let a = 2;");
            assert_eq!(summary.as_deref(), Some("Bump a."));
            service.decide("chat-1", call_id, true).unwrap();
        }
        let over = matches!(
            event,
            AssistantEvent::Done { .. } | AssistantEvent::Failed { .. }
        );
        seen.push(event);
        if over {
            break;
        }
    }
    assert!(
        matches!(seen.last(), Some(AssistantEvent::Done { .. })),
        "{seen:?}"
    );
    assert!(seen.iter().any(|event| matches!(
        event,
        AssistantEvent::Tool { name, status: ToolStatus::Done, .. } if name == "read_file"
    )));
    assert!(seen.iter().any(|event| matches!(
        event,
        AssistantEvent::ProposalDecided {
            accepted: true,
            error: None,
            ..
        }
    )));
    let text: String = seen
        .iter()
        .filter_map(|event| match event {
            AssistantEvent::Text { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "Done: a is 2.");
    assert_eq!(tools.files.lock().unwrap()["a.ts"], "let a = 2;");

    // The model was told the tools, the folder, and each result.
    let first = bodies.recv().unwrap();
    let names: Vec<&str> = first["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["read_file", "propose_edit"]);
    assert!(
        first["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("\"demo\"")
    );
    let second = bodies.recv().unwrap();
    assert_eq!(second["messages"][3]["role"], "tool");
    assert_eq!(second["messages"][3]["content"], "let a = 1;");
    let third = bodies.recv().unwrap();
    assert!(
        third["messages"][5]["content"]
            .as_str()
            .unwrap()
            .contains("accepted")
    );
}

#[test]
fn a_rejected_change_is_not_written() {
    let (port, _bodies) = fake_server(vec![
        sse(&[tool_call(
            "c1",
            "propose_edit",
            json!({ "path": "new.ts", "new_text": "export {}" }),
        )]),
        sse(&[json!({ "choices": [{ "delta": { "content": "Fine." } }] })]),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let service =
        AssistantService::with_secrets(dir.path().to_path_buf(), Arc::new(InMemory::default()));
    let mut ollama = setup(ProviderKind::Ollama);
    ollama.endpoint = Some(format!("http://127.0.0.1:{port}"));
    service.save(&ollama, None).unwrap();
    let tools = Arc::new(FakeTools::default());
    let (events_in, events) = mpsc::channel();
    let events_in = Mutex::new(events_in);
    service
        .send(
            "chat-2",
            "Add a file",
            ChatContext::default(),
            Arc::clone(&tools) as Arc<dyn WorkspaceTools>,
            Arc::new(move |event| {
                let _ = events_in.lock().unwrap().send(event);
            }),
        )
        .unwrap();
    loop {
        let event = events.recv_timeout(Duration::from_secs(10)).unwrap();
        match event {
            AssistantEvent::Proposal {
                call_id, original, ..
            } => {
                assert_eq!(original, None);
                service.decide("chat-2", &call_id, false).unwrap();
                assert!(service.decide("chat-2", &call_id, true).is_err());
            }
            AssistantEvent::Done { .. } => break,
            AssistantEvent::Failed { message, .. } => panic!("{message}"),
            _ => {}
        }
    }
    assert!(tools.files.lock().unwrap().is_empty());
}
