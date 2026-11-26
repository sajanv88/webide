//! Language features from language servers over LSP: TypeScript and
//! JavaScript from the TypeScript server that ships with the app (TypeScript
//! 7's native `tsc --lsp --stdio`), and JSON, CSS, HTML and YAML from servers
//! the app bundles, run on the person's own Node.
//!
//! The programs are the app's own; nothing from the workspace is run. Each
//! starts in the workspace folder the first time a file of its kind opens,
//! and is stopped when the session is dropped.

mod rpc;
mod servers;
mod uri;

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use ide_core::language::{
    CodeAction, Completion, CompletionKind, Completions, Definition, HoverText, LanguageServer,
    LanguageSink, LanguageStatus, Problem, ResolvedCompletion, Severity, SourceAction, TextEdit,
    TextPosition, TextRange,
};
use ide_core::{IdeError, RelPath, WorkspaceRoot};
use serde_json::{Value, json};

pub use crate::rpc::{OnRequest, RpcClient, RpcError, read_message, write_message};
pub use crate::servers::{
    FindNode, LanguageId, Program, ServerSpec, Servers, WORKSPACE_ARG, angular_workspace,
    css_language, html_language, json_language, yaml_language,
};
pub use crate::uri::{from_uri, relative_to, to_uri};

#[cfg(test)]
pub(crate) use crate::servers::schema_content as schema_content_for_test;

/// How long a server may take to start.
const START_LIMIT: Duration = Duration::from_secs(60);
/// How long one question (completion, hover…) may take.
const ASK_LIMIT: Duration = Duration::from_secs(15);
/// How long a server that sends its problems unasked has, after a change, to send them.
const PUSH_WAIT: Duration = Duration::from_secs(2);
/// At most this many completions are passed on.
pub const MAX_COMPLETIONS: usize = 1000;

fn failed(error: &RpcError) -> IdeError {
    IdeError::Io(error.to_string())
}

/// The problems a server sent unasked (`textDocument/publishDiagnostics`), by file.
type Pushed = Arc<Mutex<HashMap<RelPath, (Instant, Vec<Value>)>>>;

/// Starts one server for the workspace at `root`, as `spec` says, with `program`.
fn start_session(
    spec: &ServerSpec,
    program: &Path,
    args: &[String],
    root: &WorkspaceRoot,
    status: &LanguageSink,
) -> Result<Session, IdeError> {
    let name = spec.name;
    let fail = |message: String| {
        status(name, LanguageStatus::Failed(message.clone()));
        IdeError::Io(message)
    };
    status(name, LanguageStatus::Starting);
    let folder = root.display_path();
    // Said once, when it stops by itself (not when it's asked to).
    let stopping = Arc::new(AtomicBool::new(false));
    let (exit_status, exit_stopping) = (Arc::clone(status), Arc::clone(&stopping));
    let pushed: Pushed = Arc::new(Mutex::new(HashMap::new()));
    let (push_into, push_folder) = (Arc::clone(&pushed), folder.clone());
    let client = RpcClient::spawn(
        program,
        args,
        Path::new(&folder),
        Box::new(move |method, params| {
            if method != "textDocument/publishDiagnostics" {
                return;
            }
            let Some(path) = params
                .get("uri")
                .and_then(Value::as_str)
                .and_then(from_uri)
                .and_then(|path| relative_to(&push_folder, &path))
            else {
                return;
            };
            let items = params
                .get("diagnostics")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            push_into
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(path, (Instant::now(), items));
        }),
        servers::responder(spec),
        Box::new(move || {
            if !exit_stopping.load(Ordering::Acquire) {
                exit_status(
                    name,
                    LanguageStatus::Failed(format!("the {name} server stopped")),
                );
            }
        }),
    )
    .map_err(|error| fail(format!("couldn't start the {name} server: {error}")))?;

    let root_uri = to_uri(&folder);
    let initialized = client.request(
        "initialize",
        json!({
            "processId": std::process::id(),
            "clientInfo": { "name": "webide" },
            "rootUri": root_uri,
            "workspaceFolders": [{ "uri": root_uri, "name": root.name() }],
            "initializationOptions": spec.initialization_options,
            "capabilities": {
                "general": { "positionEncodings": ["utf-16"] },
                "textDocument": {
                    "synchronization": { "didSave": false },
                    "completion": {
                        "completionItem": {
                            "snippetSupport": false,
                            "documentationFormat": ["markdown", "plaintext"],
                            "labelDetailsSupport": true,
                            // The import a completion adds comes when it's picked.
                            "resolveSupport": {
                                "properties": ["additionalTextEdits", "detail"],
                            },
                        },
                    },
                    "codeAction": {
                        "codeActionLiteralSupport": {
                            "codeActionKind": {
                                "valueSet": ["quickfix", "source", "source.organizeImports"],
                            },
                        },
                        "isPreferredSupport": true,
                        "disabledSupport": true,
                    },
                    "hover": { "contentFormat": ["markdown", "plaintext"] },
                    "definition": { "linkSupport": true },
                    "diagnostic": {},
                    "publishDiagnostics": {},
                },
                "workspace": {
                    "configuration": true,
                    "workspaceFolders": true,
                    "workspaceEdit": { "documentChanges": true },
                    "didChangeConfiguration": {},
                },
            },
        }),
        START_LIMIT,
    );
    let capabilities = match initialized {
        Ok(result) => result.get("capabilities").cloned().unwrap_or(Value::Null),
        Err(error) => return Err(fail(format!("the {name} server didn't start: {error}"))),
    };
    let ready = client.notify("initialized", json!({})).and_then(|()| {
        for (method, params) in &spec.notifications {
            client.notify(method, params.clone())?;
        }
        // Servers that read their settings when told (JSON, YAML) are told now.
        if spec.settings.is_null() {
            Ok(())
        } else {
            client.notify(
                "workspace/didChangeConfiguration",
                json!({ "settings": spec.settings }),
            )
        }
    });
    if let Err(error) = ready {
        return Err(fail(format!("the {name} server didn't start: {error}")));
    }
    status(name, LanguageStatus::Ready);
    Ok(Session {
        client,
        folder,
        language: spec.language,
        pull: capabilities.get("diagnosticProvider").is_some(),
        pushed,
        changed: Mutex::new(HashMap::new()),
        versions: Mutex::new(HashMap::new()),
        listed: Mutex::new(Listed::default()),
        reported: Mutex::new(HashMap::new()),
        stopping,
    })
}

struct Session {
    client: RpcClient,
    /// The workspace folder, as people write it.
    folder: String,
    /// The LSP language id of a file this server handles.
    language: LanguageId,
    /// Whether problems are asked for (`textDocument/diagnostic`); otherwise
    /// the server sends them unasked, into `pushed`.
    pull: bool,
    pushed: Pushed,
    /// When each open file last changed, so an older push isn't taken as new.
    changed: Mutex<HashMap<RelPath, Instant>>,
    /// Each open file's version, as the server knows it.
    versions: Mutex<HashMap<RelPath, i64>>,
    /// The last completions listed, as the server wrote them: picking one
    /// asks the server about it again (`completionItem/resolve`).
    listed: Mutex<Listed>,
    /// Each file's problems as the server last reported them: a request for
    /// fixes sends back the ones it's about.
    reported: Mutex<HashMap<RelPath, Vec<Value>>>,
    stopping: Arc<AtomicBool>,
}

/// The last completion list: which, for which file, and its items.
#[derive(Default)]
struct Listed {
    generation: u64,
    path: Option<RelPath>,
    items: Vec<Value>,
}

/// Each `edits` list in a workspace edit, by the file it changes (as a URI).
/// `None` when it does more than edit text (creates, renames or deletes files).
fn edits_by_file(edit: &Value) -> Option<Vec<(&str, &Value)>> {
    let mut files = Vec::new();
    if let Some(changes) = edit.get("changes").and_then(Value::as_object) {
        files.extend(changes.iter().map(|(uri, edits)| (uri.as_str(), edits)));
    }
    if let Some(changes) = edit.get("documentChanges").and_then(Value::as_array) {
        for change in changes {
            let uri = change.get("textDocument")?.get("uri")?.as_str()?;
            files.push((uri, change.get("edits")?));
        }
    }
    Some(files)
}

fn text_edits(list: &Value) -> Vec<TextEdit> {
    list.as_array()
        .map(|edits| {
            edits
                .iter()
                .filter_map(|edit| {
                    Some(TextEdit {
                        range: range(edit.get("range")?)?,
                        new_text: edit.get("newText")?.as_str()?.to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Whether two ranges meet (touching counts: a cursor at a problem's end is on it).
fn meets(a: TextRange, b: TextRange) -> bool {
    a.start <= b.end && b.start <= a.end
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

    /// The edits of a workspace edit, when they're all to `path`.
    fn edits_to(&self, path: &RelPath, edit: &Value) -> Option<Vec<TextEdit>> {
        let mut edits = Vec::new();
        for (uri, list) in edits_by_file(edit)? {
            let file = relative_to(&self.folder, &from_uri(uri)?)?;
            if &file != path {
                return None;
            }
            edits.extend(text_edits(list));
        }
        Some(edits)
    }

    /// The server's code actions of the `only` kind over `range`, given `diagnostics`.
    fn code_actions(
        &self,
        path: &RelPath,
        range: TextRange,
        diagnostics: Vec<Value>,
        only: &str,
    ) -> Result<Vec<Value>, IdeError> {
        let point = |at: TextPosition| json!({ "line": at.line, "character": at.character });
        let result = self.ask(
            "textDocument/codeAction",
            json!({
                "textDocument": { "uri": self.uri(path) },
                "range": { "start": point(range.start), "end": point(range.end) },
                "context": { "diagnostics": diagnostics, "only": [only] },
            }),
        )?;
        Ok(match result {
            Value::Array(actions) => actions
                .into_iter()
                // A command alone (no edit) isn't run: only edits are made.
                .filter(|action| action.get("disabled").is_none() && action.get("edit").is_some())
                .collect(),
            _ => Vec::new(),
        })
    }

    fn mark_changed(&self, path: &RelPath) {
        self.changed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(path.clone(), Instant::now());
    }

    /// The problems the server sent for `path` since it last changed,
    /// waiting a moment for them; what it sent before, if it sends none.
    fn pushed_problems(&self, path: &RelPath) -> Vec<Value> {
        let since = self
            .changed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(path)
            .copied();
        let until = Instant::now() + PUSH_WAIT;
        loop {
            let latest = self
                .pushed
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(path)
                .cloned();
            let fresh = matches!((&latest, since), (Some((at, _)), Some(since)) if *at >= since)
                || (latest.is_some() && since.is_none());
            if fresh || Instant::now() >= until || !self.client.is_alive() {
                return latest.map(|(_, items)| items).unwrap_or_default();
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn resolve_item(&self, path: &RelPath, resolve: &str) -> Option<Value> {
        let (generation, index) = resolve.split_once(':')?;
        let (generation, index) = (
            generation.parse::<u64>().ok()?,
            index.parse::<usize>().ok()?,
        );
        let listed = self.listed.lock().unwrap_or_else(PoisonError::into_inner);
        // Only the latest list, of this file: an older one's items are gone.
        if listed.generation != generation || listed.path.as_ref() != Some(path) {
            return None;
        }
        listed.items.get(index).cloned()
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
        let Some(language) = (self.language)(path) else {
            return Ok(());
        };
        self.mark_changed(path);
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
        self.mark_changed(path);
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
        let items = if self.pull {
            let report = self.ask(
                "textDocument/diagnostic",
                json!({ "textDocument": { "uri": self.uri(path) } }),
            )?;
            report
                .get("items")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        } else {
            self.pushed_problems(path)
        };
        let problems = items.iter().filter_map(problem).collect();
        self.reported
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(path.clone(), items);
        Ok(problems)
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
        let incomplete = incomplete || items.len() > MAX_COMPLETIONS;
        let items = &items[..items.len().min(MAX_COMPLETIONS)];
        let generation = {
            let mut listed = self.listed.lock().unwrap_or_else(PoisonError::into_inner);
            listed.generation += 1;
            listed.path = Some(path.clone());
            listed.items = items.to_vec();
            listed.generation
        };
        Ok(Completions {
            incomplete,
            items: items
                .iter()
                .enumerate()
                .filter_map(|(index, item)| {
                    let mut found = completion(item)?;
                    // Only an item the server has more to say about is asked again.
                    if item.get("data").is_some() {
                        found.resolve = Some(format!("{generation}:{index}"));
                    }
                    Some(found)
                })
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

    fn resolve_completion(
        &self,
        path: &RelPath,
        resolve: &str,
    ) -> Result<ResolvedCompletion, IdeError> {
        let Some(item) = self.resolve_item(path, resolve) else {
            return Err(IdeError::NotFound);
        };
        let resolved = self.ask("completionItem/resolve", item)?;
        Ok(ResolvedCompletion {
            detail: resolved
                .get("detail")
                .and_then(Value::as_str)
                .map(str::to_owned),
            edits: resolved
                .get("additionalTextEdits")
                .map(text_edits)
                .unwrap_or_default(),
        })
    }

    fn fixes(&self, path: &RelPath, range: TextRange) -> Result<Vec<CodeAction>, IdeError> {
        // The problems there, as the server reports them now: its fixes are for
        // those, and the last report may be from before the latest typing.
        self.problems(path)?;
        let diagnostics: Vec<Value> = self
            .reported
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(path)
            .map(|items| {
                items
                    .iter()
                    .filter(|item| {
                        item.get("range")
                            .and_then(self::range)
                            .is_some_and(|at| meets(at, range))
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        if diagnostics.is_empty() {
            return Ok(Vec::new());
        }
        let actions = self.code_actions(path, range, diagnostics, "quickfix")?;
        Ok(actions
            .iter()
            .filter_map(|action| {
                Some(CodeAction {
                    title: action.get("title")?.as_str()?.to_owned(),
                    preferred: action
                        .get("isPreferred")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    // Only fixes that change this file alone: it's the one being edited.
                    edits: self.edits_to(path, action.get("edit")?)?,
                })
            })
            .filter(|action| !action.edits.is_empty())
            .collect())
    }

    fn source_action(
        &self,
        path: &RelPath,
        action: SourceAction,
    ) -> Result<Option<Vec<TextEdit>>, IdeError> {
        let start = TextPosition {
            line: 0,
            character: 0,
        };
        let kind = action.kind();
        let actions = self.code_actions(path, TextRange { start, end: start }, Vec::new(), kind)?;
        Ok(actions
            .iter()
            .find(|found| found.get("kind").and_then(Value::as_str) == Some(kind))
            .and_then(|found| self.edits_to(path, found.get("edit")?)))
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

/// A snippet's text without its places to fill in: `$1` and `$0` go,
/// `${1:name}` leaves `name`, `${1|a,b|}` leaves `a`, and `\$` is `$`.
/// Variables (`$TM_FILENAME`) go too: the editor fills in none of them.
fn snippet_text(snippet: &str) -> String {
    let chars: Vec<char> = snippet.chars().collect();
    let mut out = String::new();
    let mut at = 0;
    // How many `${…}` are open, and whether each is a choice (`|a,b|`).
    let mut open: Vec<bool> = Vec::new();
    while let Some(&c) = chars.get(at) {
        match c {
            '\\' if matches!(chars.get(at + 1), Some('$' | '}' | '\\' | '|' | ',')) => {
                out.push(chars[at + 1]);
                at += 2;
            }
            '$' if chars.get(at + 1) == Some(&'{') => {
                // `${1:` or `${name:` or `${1|`: skip to the default or the choices.
                let mut end = at + 2;
                while chars
                    .get(end)
                    .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
                {
                    end += 1;
                }
                match chars.get(end) {
                    Some(':') => {
                        open.push(false);
                        at = end + 1;
                    }
                    Some('|') => {
                        // The first choice is kept; the rest, to `|}`, go.
                        let rest: String = chars[end + 1..].iter().collect();
                        let close = rest.find("|}").unwrap_or(rest.len());
                        let first = rest[..close].split(',').next().unwrap_or("");
                        out.push_str(first);
                        at = end + 1 + rest[..close].chars().count() + 2;
                    }
                    _ => {
                        // `${1}`: nothing to keep.
                        at = chars[end..]
                            .iter()
                            .position(|c| *c == '}')
                            .map_or(chars.len(), |close| end + close + 1);
                    }
                }
            }
            '$' if chars
                .get(at + 1)
                .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_') =>
            {
                at += 1;
                while chars
                    .get(at)
                    .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
                {
                    at += 1;
                }
            }
            '}' if !open.is_empty() => {
                open.pop();
                at += 1;
            }
            _ => {
                out.push(c);
                at += 1;
            }
        }
    }
    out
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
        // A snippet (some servers send them though told not to) as plain text.
        insert: edit.or_else(|| text("insertText")).map(|insert| {
            if value.get("insertTextFormat").and_then(Value::as_u64) == Some(2) {
                snippet_text(&insert)
            } else {
                insert
            }
        }),
        // A TextEdit's range, or an InsertReplaceEdit's `insert` one.
        replace_from: value
            .get("textEdit")
            .and_then(|edit| edit.get("range").or_else(|| edit.get("insert")))
            .and_then(|range| range.get("start"))
            .and_then(position),
        sort: text("sortText"),
        source: value
            .get("labelDetails")
            .and_then(|details| details.get("description"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        edits: value
            .get("additionalTextEdits")
            .map(text_edits)
            .unwrap_or_default(),
        resolve: None,
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
