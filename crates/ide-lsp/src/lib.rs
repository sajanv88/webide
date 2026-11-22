//! Language features from a language server over LSP: for now TypeScript and
//! JavaScript, from the TypeScript server that ships with the app
//! (TypeScript 7's native `tsc --lsp --stdio`).
//!
//! The program is the app's own; nothing from the workspace is run. It starts
//! in the workspace folder, and is stopped when the session is dropped.

mod rpc;
mod uri;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use ide_core::language::{
    Completion, CompletionKind, Completions, Definition, HoverText, LanguageServer,
    LanguageServers, LanguageSink, LanguageStatus, Problem, Severity, TextPosition, TextRange,
    script_language,
};
use ide_core::{IdeError, RelPath, WorkspaceRoot};
use serde_json::{Value, json};

pub use crate::rpc::{RpcClient, RpcError, read_message, write_message};
pub use crate::uri::{from_uri, relative_to, to_uri};

/// How long the server may take to start.
const START_LIMIT: Duration = Duration::from_secs(60);
/// How long one question (completion, hover…) may take.
const ASK_LIMIT: Duration = Duration::from_secs(15);
/// At most this many completions are passed on.
pub const MAX_COMPLETIONS: usize = 1000;

/// [`LanguageServers`] for TypeScript and JavaScript.
#[derive(Debug, Clone)]
pub struct TypeScriptServers {
    program: PathBuf,
    args: Vec<String>,
}

impl TypeScriptServers {
    /// `program` is the server: TypeScript 7's `tsc`, run with `--lsp --stdio`.
    pub fn new(program: PathBuf) -> Self {
        Self {
            program,
            args: vec!["--lsp".to_owned(), "--stdio".to_owned()],
        }
    }
}

fn failed(error: &RpcError) -> IdeError {
    IdeError::Io(error.to_string())
}

impl LanguageServers for TypeScriptServers {
    fn handles(&self, path: &RelPath) -> bool {
        script_language(path).is_some()
    }

    fn start(
        &self,
        root: &WorkspaceRoot,
        status: LanguageSink,
    ) -> Result<Box<dyn LanguageServer>, IdeError> {
        if !self.program.is_file() {
            let message = format!(
                "the TypeScript language server isn't installed with this app ({})",
                self.program.display()
            );
            status(LanguageStatus::Failed(message.clone()));
            return Err(IdeError::Io(message));
        }
        status(LanguageStatus::Starting);
        let folder = root.display_path();
        // Said once, when it stops by itself (not when it's asked to).
        let stopping = Arc::new(AtomicBool::new(false));
        let (exit_status, exit_stopping) = (Arc::clone(&status), Arc::clone(&stopping));
        let client = RpcClient::spawn(
            &self.program,
            &self.args,
            std::path::Path::new(&folder),
            Box::new(|_method, _params| {}),
            Box::new(move || {
                if !exit_stopping.load(Ordering::Acquire) {
                    exit_status(LanguageStatus::Failed(
                        "the TypeScript server stopped".to_owned(),
                    ));
                }
            }),
        )
        .map_err(|error| {
            let message = format!("couldn't start the TypeScript server: {error}");
            status(LanguageStatus::Failed(message.clone()));
            IdeError::Io(message)
        })?;

        let root_uri = to_uri(&folder);
        let initialized = client.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "clientInfo": { "name": "webide" },
                "rootUri": root_uri,
                "workspaceFolders": [{ "uri": root_uri, "name": root.name() }],
                "capabilities": {
                    "general": { "positionEncodings": ["utf-16"] },
                    "textDocument": {
                        "synchronization": { "didSave": false },
                        "completion": {
                            "completionItem": {
                                "snippetSupport": false,
                                "documentationFormat": ["markdown", "plaintext"],
                            },
                        },
                        "hover": { "contentFormat": ["markdown", "plaintext"] },
                        "definition": { "linkSupport": true },
                        "diagnostic": {},
                        "publishDiagnostics": {},
                    },
                    "workspace": { "configuration": true, "workspaceFolders": true },
                },
            }),
            START_LIMIT,
        );
        if let Err(error) = initialized.and_then(|_| client.notify("initialized", json!({}))) {
            let message = format!("the TypeScript server didn't start: {error}");
            status(LanguageStatus::Failed(message.clone()));
            return Err(IdeError::Io(message));
        }
        status(LanguageStatus::Ready);
        Ok(Box::new(Session {
            client,
            folder,
            versions: Mutex::new(HashMap::new()),
            stopping,
        }))
    }
}

struct Session {
    client: RpcClient,
    /// The workspace folder, as people write it.
    folder: String,
    /// Each open file's version, as the server knows it.
    versions: Mutex<HashMap<RelPath, i64>>,
    stopping: Arc<AtomicBool>,
}

impl Session {
    fn uri(&self, path: &RelPath) -> String {
        to_uri(&format!(
            "{}/{}",
            self.folder.trim_end_matches(['/', '\\']),
            path.as_str()
        ))
    }

    fn document(&self, path: &RelPath, at: TextPosition) -> Value {
        json!({
            "textDocument": { "uri": self.uri(path) },
            "position": { "line": at.line, "character": at.character },
        })
    }

    fn ask(&self, method: &str, params: Value) -> Result<Value, IdeError> {
        self.client
            .request(method, params, ASK_LIMIT)
            .map_err(|error| failed(&error))
    }

    fn definition_of(&self, uri: &str, range: TextRange) -> Option<Definition> {
        let path = from_uri(uri)?;
        Some(match relative_to(&self.folder, &path) {
            Some(path) => Definition::Workspace { path, range },
            None => Definition::Outside { path, range },
        })
    }
}

impl LanguageServer for Session {
    fn open(&self, path: &RelPath, text: &str) -> Result<(), IdeError> {
        let Some(language) = script_language(path) else {
            return Ok(());
        };
        let reopened = self
            .versions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(path);
        if reopened {
            return self.change(path, text);
        }
        self.versions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(path.clone(), 1);
        self.client
            .notify(
                "textDocument/didOpen",
                json!({
                    "textDocument": {
                        "uri": self.uri(path),
                        "languageId": language,
                        "version": 1,
                        "text": text,
                    },
                }),
            )
            .map_err(|error| failed(&error))
    }

    fn change(&self, path: &RelPath, text: &str) -> Result<(), IdeError> {
        let version = {
            let mut versions = self.versions.lock().unwrap_or_else(PoisonError::into_inner);
            let Some(version) = versions.get_mut(path) else {
                drop(versions);
                return self.open(path, text);
            };
            *version += 1;
            *version
        };
        // The whole text each time: simple, and fast enough for a file being typed in.
        self.client
            .notify(
                "textDocument/didChange",
                json!({
                    "textDocument": { "uri": self.uri(path), "version": version },
                    "contentChanges": [{ "text": text }],
                }),
            )
            .map_err(|error| failed(&error))
    }

    fn close(&self, path: &RelPath) -> Result<(), IdeError> {
        let was_open = self
            .versions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(path)
            .is_some();
        if !was_open {
            return Ok(());
        }
        self.client
            .notify(
                "textDocument/didClose",
                json!({ "textDocument": { "uri": self.uri(path) } }),
            )
            .map_err(|error| failed(&error))
    }

    fn problems(&self, path: &RelPath) -> Result<Vec<Problem>, IdeError> {
        let report = self.ask(
            "textDocument/diagnostic",
            json!({ "textDocument": { "uri": self.uri(path) } }),
        )?;
        Ok(report
            .get("items")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(problem).collect())
            .unwrap_or_default())
    }

    fn completions(&self, path: &RelPath, at: TextPosition) -> Result<Completions, IdeError> {
        let result = self.ask("textDocument/completion", self.document(path, at))?;
        let (items, incomplete) = match &result {
            Value::Array(items) => (items.as_slice(), false),
            Value::Object(list) => (
                list.get("items")
                    .and_then(Value::as_array)
                    .map_or(&[][..], Vec::as_slice),
                list.get("isIncomplete")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            ),
            _ => (&[][..], false),
        };
        Ok(Completions {
            incomplete: incomplete || items.len() > MAX_COMPLETIONS,
            items: items
                .iter()
                .take(MAX_COMPLETIONS)
                .filter_map(completion)
                .collect(),
        })
    }

    fn hover(&self, path: &RelPath, at: TextPosition) -> Result<Option<HoverText>, IdeError> {
        let result = self.ask("textDocument/hover", self.document(path, at))?;
        let markdown = hover_markdown(result.get("contents").unwrap_or(&Value::Null));
        Ok((!markdown.trim().is_empty()).then_some(HoverText { markdown }))
    }

    fn definition(&self, path: &RelPath, at: TextPosition) -> Result<Vec<Definition>, IdeError> {
        let result = self.ask("textDocument/definition", self.document(path, at))?;
        let list = match result {
            Value::Array(list) => list,
            Value::Null => Vec::new(),
            single => vec![single],
        };
        Ok(list
            .iter()
            .filter_map(|target| {
                // A Location (`uri`, `range`) or a LocationLink (`targetUri`, `targetSelectionRange`).
                let uri = target
                    .get("uri")
                    .or_else(|| target.get("targetUri"))?
                    .as_str()?;
                let range = range(
                    target
                        .get("range")
                        .or_else(|| target.get("targetSelectionRange"))?,
                )?;
                self.definition_of(uri, range)
            })
            .collect())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        // Asked to leave; the client kills it if it doesn't.
        let _ = self
            .client
            .request("shutdown", Value::Null, Duration::from_secs(2));
        let _ = self.client.notify("exit", Value::Null);
    }
}

fn position(value: &Value) -> Option<TextPosition> {
    Some(TextPosition {
        line: u32::try_from(value.get("line")?.as_u64()?).ok()?,
        character: u32::try_from(value.get("character")?.as_u64()?).ok()?,
    })
}

fn range(value: &Value) -> Option<TextRange> {
    Some(TextRange {
        start: position(value.get("start")?)?,
        end: position(value.get("end")?)?,
    })
}

fn problem(value: &Value) -> Option<Problem> {
    Some(Problem {
        range: range(value.get("range")?)?,
        severity: match value.get("severity").and_then(Value::as_u64) {
            Some(2) => Severity::Warning,
            Some(3) => Severity::Info,
            Some(4) => Severity::Hint,
            _ => Severity::Error,
        },
        message: value.get("message")?.as_str()?.to_owned(),
        source: value
            .get("source")
            .and_then(Value::as_str)
            .map(str::to_owned),
        code: value.get("code").and_then(|code| match code {
            Value::String(text) => Some(text.clone()),
            Value::Number(number) => Some(number.to_string()),
            _ => None,
        }),
    })
}

fn completion(value: &Value) -> Option<Completion> {
    let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
    let edit = value
        .get("textEdit")
        .and_then(|edit| edit.get("newText"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    Some(Completion {
        label: text("label")?,
        kind: completion_kind(value.get("kind").and_then(Value::as_u64)),
        detail: text("detail"),
        insert: edit.or_else(|| text("insertText")),
        sort: text("sortText"),
    })
}

/// LSP's `CompletionItemKind` numbers.
fn completion_kind(kind: Option<u64>) -> CompletionKind {
    match kind {
        Some(1) => CompletionKind::Text,
        Some(2) => CompletionKind::Method,
        Some(3) => CompletionKind::Function,
        Some(4) => CompletionKind::Constructor,
        Some(5) => CompletionKind::Field,
        Some(6) => CompletionKind::Variable,
        Some(7) => CompletionKind::Class,
        Some(8) => CompletionKind::Interface,
        Some(9) => CompletionKind::Module,
        Some(10) => CompletionKind::Property,
        Some(13) => CompletionKind::Enum,
        Some(14) => CompletionKind::Keyword,
        Some(15) => CompletionKind::Snippet,
        Some(17) => CompletionKind::File,
        Some(19) => CompletionKind::Folder,
        Some(20) => CompletionKind::EnumMember,
        Some(21) => CompletionKind::Constant,
        Some(25) => CompletionKind::TypeParameter,
        _ => CompletionKind::Other,
    }
}

/// A hover's `contents` (MarkupContent, a MarkedString, or a list of them) as Markdown.
fn hover_markdown(contents: &Value) -> String {
    match contents {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .map(hover_markdown)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"),
        Value::Object(object) => {
            let value = object.get("value").and_then(Value::as_str).unwrap_or("");
            match object.get("language").and_then(Value::as_str) {
                // A MarkedString with a language is code.
                Some(language) => format!("```{language}\n{value}\n```"),
                None => value.to_owned(),
            }
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests;
