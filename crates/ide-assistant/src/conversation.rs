//! One conversation: the person's message, then the model's turns until it
//! stops calling tools. Read tools run at once; a proposed change waits for
//! the person, and is written only if they accept it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ide_core::agent::WorkspaceTools;
use ide_core::assistant::{AssistantEvent, AssistantSink, ChatContext, ToolStatus};
use serde_json::{Value, json};
use tokio::sync::{oneshot, watch};

use crate::providers::{self, Connection, Message, Part, Request, Role, ToolDef};

/// The most model turns one message may take.
const MAX_STEPS: usize = 40;
/// The most of a tool's answer the model is given, in characters.
const MAX_TOOL_OUTPUT: usize = 60_000;
/// The most of it shown to the person.
const MAX_SHOWN_OUTPUT: usize = 2_000;
/// The tool that proposes a change.
pub(crate) const PROPOSE: &str = "propose_edit";

/// A conversation's messages, and what its reply in progress waits on.
#[derive(Default)]
pub(crate) struct Chat {
    pub history: Mutex<Vec<Message>>,
    /// Set while a reply is coming: setting it to `true` stops it.
    pub cancel: Mutex<Option<watch::Sender<bool>>>,
    /// Proposals waiting for the person, by call id.
    pub pending: Mutex<HashMap<String, oneshot::Sender<bool>>>,
}

impl Chat {
    fn commit(&self, messages: &[Message]) {
        if let Ok(mut history) = self.history.lock() {
            *history = messages.to_vec();
        }
    }
}

/// What the model is told about its place and its tools.
pub(crate) fn system_prompt(context: &ChatContext) -> String {
    let folder = context.workspace.as_deref().map_or_else(
        || "No folder is open.".to_owned(),
        |name| format!("The open folder is \"{name}\"."),
    );
    let looking = context
        .active_path
        .as_deref()
        .map(|path| format!(" The person has {path} open in front of them."))
        .unwrap_or_default();
    format!(
        "You are the AI assistant in webide, a code editor, helping with the code in the folder \
         open in it. {folder}{looking}\n\n\
         Look at the code with the tools before answering questions about it: don't guess what a \
         file holds. Paths are relative to the folder's root. Files are read from the disk.\n\n\
         To change a file, call {PROPOSE}: the person sees the change as a diff and accepts or \
         rejects it. Propose one change per call, and keep old_text short but unique. Never say \
         a change is made until {PROPOSE} says it was accepted. You can't run commands, tests or \
         builds: say what to run instead.\n\n\
         Answer in Markdown, briefly."
    )
}

/// The tool that proposes a change, as the model is told about it.
pub(crate) fn propose_tool() -> ToolDef {
    ToolDef {
        name: PROPOSE.to_owned(),
        description: "Proposes a change to one file in the folder. The person sees it as a diff \
                      and accepts or rejects it; the answer says which. With old_text, its one \
                      occurrence in the file is replaced by new_text. Without it, new_text is the \
                      file's whole new text, or a new file's."
            .to_owned(),
        schema: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The file, relative to the folder's root (\"src/main.ts\")."
                },
                "old_text": {
                    "type": "string",
                    "description": "Text the file has exactly once, to replace. Leave it out to write the whole file."
                },
                "new_text": {
                    "type": "string",
                    "description": "What replaces old_text, or the whole file."
                },
                "summary": {
                    "type": "string",
                    "description": "One sentence: what the change does."
                }
            },
            "required": ["path", "new_text"]
        }),
    }
}

/// The person's message, added to the last one when that's theirs too
/// (after a reply that failed): the providers want turns to alternate.
pub(crate) fn add_user(messages: &mut Vec<Message>, parts: Vec<Part>) {
    match messages.last_mut() {
        Some(last) if last.role == Role::User => last.parts.extend(parts),
        _ => messages.push(Message {
            role: Role::User,
            parts,
        }),
    }
}

/// `text`, cut to `max` characters with a note.
fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let kept: String = text.chars().take(max).collect();
    format!("{kept}\n… (cut: the rest is longer than {max} characters)")
}

/// `new_text` in place of `old_text`'s one occurrence in `original`, or the
/// whole of it without `old_text`.
pub(crate) fn apply_change(
    path: &str,
    original: Option<&str>,
    old_text: Option<&str>,
    new_text: &str,
) -> Result<String, String> {
    let Some(old_text) = old_text.filter(|old| !old.is_empty()) else {
        return Ok(new_text.to_owned());
    };
    let Some(original) = original else {
        return Err(format!(
            "{path} doesn't exist: leave old_text out to create it"
        ));
    };
    match original.matches(old_text).count() {
        1 => Ok(original.replacen(old_text, new_text, 1)),
        0 => Err(format!(
            "old_text isn't in {path}: read the file again and copy the text exactly"
        )),
        count => Err(format!(
            "old_text is in {path} {count} times: include more of the lines around it"
        )),
    }
}

/// Everything one message needs.
pub(crate) struct Turn {
    pub conversation: String,
    pub connection: Connection,
    pub chat: Arc<Chat>,
    pub context: ChatContext,
    pub tools: Arc<dyn WorkspaceTools>,
    pub sink: AssistantSink,
    pub cancel: watch::Receiver<bool>,
}

impl Turn {
    fn emit(&self, event: AssistantEvent) {
        (self.sink)(event);
    }

    fn cancelled(&self) -> bool {
        *self.cancel.borrow()
    }

    /// Resolves once the person stops the reply.
    async fn stopped(&self) {
        let mut cancel = self.cancel.clone();
        let _ = cancel.wait_for(|stop| *stop).await;
    }
}

/// Answers `text`; every outcome ends with `Done` or `Failed`.
pub(crate) async fn run(turn: Turn, text: String) {
    let conversation = turn.conversation.clone();
    let outcome = answer(&turn, text).await;
    if let Ok(mut cancel) = turn.chat.cancel.lock() {
        *cancel = None;
    }
    if let Ok(mut pending) = turn.chat.pending.lock() {
        pending.clear();
    }
    turn.emit(match outcome {
        Ok(()) => AssistantEvent::Done { conversation },
        Err(message) => AssistantEvent::Failed {
            conversation,
            message,
        },
    });
}

async fn answer(turn: &Turn, text: String) -> Result<(), String> {
    let tools = Arc::clone(&turn.tools);
    let mut defs: Vec<ToolDef> = tokio::task::spawn_blocking(move || tools.read_tools())
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|spec| ToolDef {
            schema: serde_json::from_str(&spec.input_schema)
                .unwrap_or_else(|_| json!({ "type": "object" })),
            name: spec.name,
            description: spec.description,
        })
        .collect();
    defs.push(propose_tool());
    let system = system_prompt(&turn.context);

    let mut messages = turn
        .chat
        .history
        .lock()
        .map(|history| history.clone())
        .unwrap_or_default();
    add_user(&mut messages, vec![Part::Text(text)]);
    turn.chat.commit(&messages);

    for _ in 0..MAX_STEPS {
        let mut streamed = String::new();
        let request = Request {
            system: &system,
            messages: &messages,
            tools: &defs,
        };
        let result = {
            let mut on_text = |delta: &str| {
                streamed.push_str(delta);
                turn.emit(AssistantEvent::Text {
                    conversation: turn.conversation.clone(),
                    delta: delta.to_owned(),
                });
            };
            tokio::select! {
                result = providers::stream(&turn.connection, &request, &mut on_text) => result,
                () = turn.stopped() => Err("Stopped.".to_owned()),
            }
        };
        let reply = match result {
            Ok(reply) => reply,
            Err(error) => {
                // What the person saw stays in the conversation.
                if !streamed.is_empty() {
                    messages.push(Message {
                        role: Role::Assistant,
                        parts: vec![Part::Text(streamed)],
                    });
                    turn.chat.commit(&messages);
                }
                return Err(error);
            }
        };
        let uses: Vec<(String, String, Value)> = reply
            .parts
            .iter()
            .filter_map(|part| match part {
                Part::ToolUse {
                    id, name, input, ..
                } => Some((id.clone(), name.clone(), input.clone())),
                _ => None,
            })
            .collect();
        if !reply.parts.is_empty() {
            messages.push(Message {
                role: Role::Assistant,
                parts: reply.parts,
            });
        }
        if uses.is_empty() {
            turn.chat.commit(&messages);
            return Ok(());
        }
        let mut results = Vec::new();
        for (id, name, input) in uses {
            let (output, is_error) = if turn.cancelled() {
                ("Stopped by the person.".to_owned(), true)
            } else if name == PROPOSE {
                propose(turn, &id, &input).await
            } else {
                call_tool(turn, &id, &name, &input).await
            };
            results.push(Part::ToolResult {
                id,
                name,
                output,
                is_error,
            });
        }
        messages.push(Message {
            role: Role::User,
            parts: results,
        });
        turn.chat.commit(&messages);
        if turn.cancelled() {
            return Err("Stopped.".to_owned());
        }
    }
    Err(format!(
        "Stopped after {MAX_STEPS} steps: send a message to carry on."
    ))
}

/// Runs a read tool: its answer for the model, and whether it failed.
async fn call_tool(turn: &Turn, id: &str, name: &str, input: &Value) -> (String, bool) {
    let arguments = input.to_string();
    let event = |status, output| AssistantEvent::Tool {
        conversation: turn.conversation.clone(),
        call_id: id.to_owned(),
        name: name.to_owned(),
        input: arguments.clone(),
        status,
        output,
    };
    turn.emit(event(ToolStatus::Running, None));
    let tools = Arc::clone(&turn.tools);
    let (tool, args) = (name.to_owned(), arguments.clone());
    let result = tokio::task::spawn_blocking(move || tools.call(&tool, &args))
        .await
        .unwrap_or_else(|_| Err("the IDE failed while doing this".to_owned()));
    match result {
        Ok(output) => {
            turn.emit(event(
                ToolStatus::Done,
                Some(shorten(&output, MAX_SHOWN_OUTPUT)),
            ));
            (shorten(&output, MAX_TOOL_OUTPUT), false)
        }
        Err(error) => {
            turn.emit(event(ToolStatus::Failed, Some(error.clone())));
            (error, true)
        }
    }
}

/// Shows a proposed change and waits for the person: the answer for the
/// model, and whether it's an error.
async fn propose(turn: &Turn, id: &str, input: &Value) -> (String, bool) {
    let text = |key: &str| input[key].as_str().map(str::to_owned);
    let fail = |message: String| {
        turn.emit(AssistantEvent::Tool {
            conversation: turn.conversation.clone(),
            call_id: id.to_owned(),
            name: PROPOSE.to_owned(),
            input: input.to_string(),
            status: ToolStatus::Failed,
            output: Some(message.clone()),
        });
        (message, true)
    };
    let (Some(path), Some(new_text)) = (text("path"), text("new_text")) else {
        return fail("path and new_text are needed".to_owned());
    };
    let tools = Arc::clone(&turn.tools);
    let wanted = path.clone();
    let read = tokio::task::spawn_blocking(move || tools.read_text(&wanted))
        .await
        .unwrap_or_else(|_| Err("the IDE failed while doing this".to_owned()));
    let (path, original) = match read {
        Ok(found) => found,
        Err(error) => return fail(error),
    };
    let proposed = match apply_change(
        &path,
        original.as_deref(),
        text("old_text").as_deref(),
        &new_text,
    ) {
        Ok(proposed) => proposed,
        Err(error) => return fail(error),
    };
    if original.as_deref() == Some(proposed.as_str()) {
        return fail(format!("that leaves {path} as it is"));
    }

    let (decided, decision) = oneshot::channel();
    if let Ok(mut pending) = turn.chat.pending.lock() {
        pending.insert(id.to_owned(), decided);
    }
    turn.emit(AssistantEvent::Proposal {
        conversation: turn.conversation.clone(),
        call_id: id.to_owned(),
        path: path.clone(),
        original,
        proposed: proposed.clone(),
        summary: text("summary"),
    });
    let accepted = tokio::select! {
        decision = decision => decision.unwrap_or(false),
        () = turn.stopped() => false,
    };
    let decided = |accepted, error| {
        turn.emit(AssistantEvent::ProposalDecided {
            conversation: turn.conversation.clone(),
            call_id: id.to_owned(),
            accepted,
            error,
        });
    };
    if !accepted {
        decided(false, None);
        return (
            format!(
                "The person rejected this change, so {path} wasn't changed. Ask what they'd \
                 like instead, unless they said."
            ),
            false,
        );
    }
    let tools = Arc::clone(&turn.tools);
    let target = path.clone();
    let written = tokio::task::spawn_blocking(move || tools.write_text(&target, &proposed))
        .await
        .unwrap_or_else(|_| Err("the IDE failed while doing this".to_owned()));
    match written {
        Ok(()) => {
            decided(true, None);
            (format!("The person accepted it: {path} is changed."), false)
        }
        Err(error) => {
            decided(true, Some(error.clone()));
            (
                format!("The person accepted it, but it couldn't be written: {error}"),
                true,
            )
        }
    }
}
