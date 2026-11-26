//! Which servers there are, the files each one handles, and starting them:
//! each the first time a file of its kind is opened in a workspace.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use ide_core::language::{
    CodeAction, Completions, Definition, HoverText, LanguageServer, LanguageServers, LanguageSink,
    LanguageStatus, Problem, ResolvedCompletion, SourceAction, TextEdit, TextPosition, TextRange,
    script_language,
};
use ide_core::{IdeError, RelPath, WorkspaceRoot};
use serde_json::{Value, json};

use crate::rpc::{OnRequest, answer_server_request};
use crate::uri::to_uri;
use crate::{Session, start_session};

/// The LSP language id of a file a server handles, by its name; `None` for others.
pub type LanguageId = fn(&RelPath) -> Option<&'static str>;

/// Finds the person's `node`, each time a server needs it (so one installed
/// since is found).
pub type FindNode = Arc<dyn Fn() -> Option<PathBuf> + Send + Sync>;

/// What runs a server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Program {
    /// A program of the app's own.
    Native(PathBuf),
    /// A script of the app's own, run on the person's `node`.
    Node(PathBuf),
}

/// One language server: what it's called, how it runs, the files it handles
/// and the settings it's given.
#[derive(Debug, Clone)]
pub struct ServerSpec {
    /// For the person: "TypeScript", "JSON".
    pub name: &'static str,
    pub program: Program,
    pub args: Vec<String>,
    pub language: LanguageId,
    /// `initialize`'s `initializationOptions`.
    pub initialization_options: Value,
    /// Its settings, by section (`json`, `yaml`…): sent once it's started
    /// (`workspace/didChangeConfiguration`) and given when it asks
    /// (`workspace/configuration`).
    pub settings: Value,
    /// The folder of the schemas the app bundles, read by name when the
    /// server asks for a SchemaStore address (`vscode/content`): nothing is
    /// downloaded.
    pub schemas: Option<PathBuf>,
    /// Notifications sent once it has started (method, params).
    pub notifications: Vec<(&'static str, Value)>,
    /// Whether it serves the workspace at this folder; every one when absent.
    /// One that doesn't leaves its files to the next server that handles them.
    pub applies: Option<fn(&Path) -> bool>,
    /// Files it needs from the project (relative to its folder), and what
    /// to tell the person when one is missing.
    pub needs: Option<(&'static [&'static str], &'static str)>,
}

/// In a server's `args`, the workspace's folder.
pub const WORKSPACE_ARG: &str = "{workspace}";

/// Whether the folder is an Angular project: `angular.json`, or
/// `@angular/core` in its `package.json`.
pub fn angular_workspace(folder: &Path) -> bool {
    folder.join("angular.json").is_file()
        || std::fs::read_to_string(folder.join("package.json"))
            .is_ok_and(|text| text.contains("\"@angular/core\""))
}

/// SchemaStore's schemas the JSON server checks files against, and the file names each is for.
const JSON_SCHEMAS: &[(&str, &[&str])] = &[
    ("package", &["package.json"]),
    (
        "tsconfig",
        &["tsconfig.json", "tsconfig.*.json", "tsconfig-*.json"],
    ),
    ("jsconfig", &["jsconfig.json", "jsconfig.*.json"]),
    ("eslintrc", &[".eslintrc", ".eslintrc.json"]),
    ("prettierrc", &[".prettierrc", ".prettierrc.json"]),
    (
        "babelrc",
        &[".babelrc", ".babelrc.json", "babel.config.json"],
    ),
    ("stylelintrc", &[".stylelintrc", ".stylelintrc.json"]),
    ("lerna", &["lerna.json"]),
    ("nest-cli", &["nest-cli.json", ".nestcli.json"]),
];

/// SchemaStore's schemas the YAML server checks files against, and the files each is for.
const YAML_SCHEMAS: &[(&str, &[&str])] = &[
    (
        "github-workflow",
        &[".github/workflows/*.yml", ".github/workflows/*.yaml"],
    ),
    ("github-action", &["action.yml", "action.yaml"]),
    (
        "dependabot-2.0",
        &[".github/dependabot.yml", ".github/dependabot.yaml"],
    ),
    ("pnpm-workspace", &["pnpm-workspace.yaml"]),
];

/// The addresses SchemaStore serves its schemas at.
const SCHEMASTORE: &[&str] = &[
    "https://json.schemastore.org/",
    "http://json.schemastore.org/",
    "https://www.schemastore.org/",
    "http://www.schemastore.org/",
];

/// A file's extension, in lower case.
fn extension(path: &RelPath) -> Option<String> {
    Some(path.file_name()?.rsplit_once('.')?.1.to_ascii_lowercase())
}

/// JSON, or JSON with comments for the files that allow them (`tsconfig.json`…).
pub fn json_language(path: &RelPath) -> Option<&'static str> {
    let name = path.file_name()?.to_ascii_lowercase();
    match extension(path)?.as_str() {
        "jsonc" => Some("jsonc"),
        "json"
            if name.starts_with("tsconfig")
                || name.starts_with("jsconfig")
                || name.starts_with(".eslintrc")
                || name.starts_with(".babelrc")
                || name == "devcontainer.json"
                || name == ".devcontainer.json"
                || name == "settings.json"
                || name == "launch.json"
                || name == "tasks.json"
                || name == "extensions.json" =>
        {
            Some("jsonc")
        }
        "json" => Some("json"),
        _ => None,
    }
}

pub fn css_language(path: &RelPath) -> Option<&'static str> {
    match extension(path)?.as_str() {
        "css" => Some("css"),
        "scss" => Some("scss"),
        "less" => Some("less"),
        _ => None,
    }
}

pub fn html_language(path: &RelPath) -> Option<&'static str> {
    matches!(extension(path)?.as_str(), "html" | "htm").then_some("html")
}

pub fn yaml_language(path: &RelPath) -> Option<&'static str> {
    matches!(extension(path)?.as_str(), "yaml" | "yml").then_some("yaml")
}

/// A path without Windows' `\\?\` prefix, which an app's resource folder can
/// have: a server that makes a URL of it (the YAML server does) can't read
/// one. Any other path, as it is.
pub fn plain_path(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => path,
    }
}

/// The bundled schemas that exist, of `list`, by name.
fn bundled<'a>(dir: &Path, list: &'a [(&'a str, &'a [&'a str])]) -> Vec<(&'a str, &'a [&'a str])> {
    list.iter()
        .filter(|(name, _)| dir.join(format!("{name}.json")).is_file())
        .copied()
        .collect()
}

impl ServerSpec {
    /// Angular's templates (`.html`) in an Angular project: Angular's own
    /// server (`script`), on Node. It loads TypeScript and
    /// `@angular/language-service` from the project's `node_modules` (the
    /// one server here that runs the workspace's code), so it needs the
    /// project's packages installed. Listed before HTML, which then has the
    /// `.html` files of every other project.
    pub fn angular(script: PathBuf) -> Self {
        let script = plain_path(script);
        // The language service the app bundles, after the project's own.
        let bundled = script.with_file_name("angular");
        Self {
            name: "Angular",
            program: Program::Node(script),
            args: vec![
                "--stdio".to_owned(),
                "--tsProbeLocations".to_owned(),
                WORKSPACE_ARG.to_owned(),
                "--ngProbeLocations".to_owned(),
                format!("{WORKSPACE_ARG},{}", bundled.to_string_lossy()),
            ],
            language: html_language,
            initialization_options: Value::Null,
            settings: Value::Null,
            schemas: None,
            notifications: Vec::new(),
            applies: Some(angular_workspace),
            needs: Some((
                &["node_modules/typescript/package.json"],
                "Angular uses the project's own TypeScript: install the project's packages (npm install, pnpm install…), then restart the language servers",
            )),
        }
    }

    /// TypeScript and JavaScript: TypeScript 7's `tsc`, run with `--lsp --stdio`.
    pub fn typescript(program: PathBuf) -> Self {
        Self {
            name: "TypeScript",
            program: Program::Native(program),
            args: vec!["--lsp".to_owned(), "--stdio".to_owned()],
            language: script_language,
            initialization_options: Value::Null,
            settings: Value::Null,
            schemas: None,
            notifications: Vec::new(),
            applies: None,
            needs: None,
        }
    }

    /// JSON: VS Code's JSON server (`script`), checking well-known files
    /// against the schemas in `schemas`.
    pub fn json(script: PathBuf, schemas: PathBuf) -> Self {
        let (script, schemas) = (plain_path(script), plain_path(schemas));
        let associations: Vec<Value> = bundled(&schemas, JSON_SCHEMAS)
            .into_iter()
            .map(|(name, files)| {
                json!({ "fileMatch": files, "url": format!("https://json.schemastore.org/{name}.json") })
            })
            .collect();
        Self {
            name: "JSON",
            program: Program::Node(script),
            args: vec!["--stdio".to_owned()],
            language: json_language,
            initialization_options: json!({
                "provideFormatter": false,
                // Others (https) are asked of the app, which has them bundled.
                "handledSchemaProtocols": ["file"],
            }),
            settings: json!({
                "json": {
                    "validate": { "enable": true },
                    "schemas": associations,
                    "format": { "enable": false },
                },
                "http": { "proxy": "", "proxyStrictSSL": true },
            }),
            schemas: Some(schemas),
            notifications: Vec::new(),
            applies: None,
            needs: None,
        }
    }

    /// CSS, SCSS and Less: VS Code's CSS server.
    pub fn css(script: PathBuf) -> Self {
        let script = plain_path(script);
        Self {
            name: "CSS",
            program: Program::Node(script),
            args: vec!["--stdio".to_owned()],
            language: css_language,
            initialization_options: json!({ "provideFormatter": false }),
            settings: json!({
                "css": { "validate": true },
                "scss": { "validate": true },
                "less": { "validate": true },
            }),
            schemas: None,
            notifications: Vec::new(),
            applies: None,
            needs: None,
        }
    }

    /// HTML, with the CSS and JavaScript inside it: VS Code's HTML server.
    pub fn html(script: PathBuf) -> Self {
        let script = plain_path(script);
        Self {
            name: "HTML",
            program: Program::Node(script),
            args: vec!["--stdio".to_owned()],
            language: html_language,
            initialization_options: json!({
                "provideFormatter": false,
                "embeddedLanguages": { "css": true, "javascript": true },
            }),
            settings: json!({
                "html": { "validate": { "scripts": true, "styles": true }, "suggest": { "html5": true } },
                "css": { "validate": true },
                "javascript": { "validate": { "enable": true } },
            }),
            schemas: None,
            notifications: Vec::new(),
            applies: None,
            needs: None,
        }
    }

    /// YAML: Red Hat's server, checking well-known files (GitHub workflows,
    /// Compose files…) against the schemas in `schemas`. It never fetches
    /// SchemaStore's catalog.
    pub fn yaml(script: PathBuf, schemas: PathBuf) -> Self {
        let (script, schemas) = (plain_path(script), plain_path(schemas));
        let l10n = script.with_file_name("yaml-l10n");
        let associations: serde_json::Map<String, Value> = bundled(&schemas, YAML_SCHEMAS)
            .into_iter()
            .map(|(name, files)| {
                let uri = to_uri(&schemas.join(format!("{name}.json")).to_string_lossy());
                (uri, json!(files))
            })
            .collect();
        Self {
            name: "YAML",
            program: Program::Node(script),
            args: vec!["--stdio".to_owned()],
            language: yaml_language,
            // Its messages, copied beside it (`yaml-l10n/bundle.l10n.json`).
            initialization_options: json!({
                "l10nPath": l10n.to_string_lossy(),
            }),
            settings: json!({
                "yaml": {
                    "validate": true,
                    "hover": true,
                    "completion": true,
                    "format": { "enable": false },
                    "schemaStore": { "enable": false },
                    "schemas": associations,
                    "keyOrdering": false,
                },
                "http": { "proxy": "", "proxyStrictSSL": true },
            }),
            schemas: Some(schemas),
            // Schemas at http(s) addresses (a file's own `$schema=` modeline
            // included) are asked of the app, which answers only from the
            // bundle: the server fetches nothing itself.
            notifications: vec![("yaml/registerContentRequest", Value::Null)],
            applies: None,
            needs: None,
        }
    }
}

/// How a server answers the requests a server makes: its settings, and the
/// bundled schemas it asks for by address.
pub(crate) fn responder(spec: &ServerSpec) -> OnRequest {
    let (settings, schemas) = (spec.settings.clone(), spec.schemas.clone());
    Box::new(move |method, params| match method {
        // A schema by address: only a bundled one. Nothing is fetched.
        "vscode/content" => schema_content(schemas.as_deref(), params)
            .map(Value::String)
            .ok_or_else(|| (-32603, "that schema isn't bundled with the app".to_owned())),
        // A schema of another kind of address (YAML's): none.
        "custom/schema/content" => {
            Err((-32603, "that schema isn't bundled with the app".to_owned()))
        }
        _ => Ok(answer_server_request(method, params, &settings)),
    })
}

/// The text of a bundled SchemaStore schema, by the address a server names.
pub(crate) fn schema_content(dir: Option<&Path>, params: Option<&Value>) -> Option<String> {
    let uri = match params? {
        Value::String(uri) => uri.as_str(),
        Value::Array(list) => list.first()?.as_str()?,
        Value::Object(object) => object.get("uri")?.as_str()?,
        _ => return None,
    };
    let rest = SCHEMASTORE
        .iter()
        .find_map(|prefix| uri.strip_prefix(prefix))?;
    let name = rest.split(['?', '#']).next()?;
    let name = name.strip_suffix(".json").unwrap_or(name);
    // A plain file name: no folder, no way out of the schemas folder.
    let plain = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !name.starts_with('.');
    if !plain {
        return None;
    }
    std::fs::read_to_string(dir?.join(format!("{name}.json"))).ok()
}

/// The servers there are: each file goes to the first that handles it.
pub struct Servers {
    specs: Arc<[ServerSpec]>,
    find_node: FindNode,
}

impl Servers {
    pub fn new(specs: Vec<ServerSpec>, find_node: FindNode) -> Self {
        Self {
            specs: specs.into(),
            find_node,
        }
    }
}

impl LanguageServers for Servers {
    fn handles(&self, path: &RelPath) -> bool {
        self.specs
            .iter()
            .any(|spec| (spec.language)(path).is_some())
    }

    fn start(
        &self,
        root: &WorkspaceRoot,
        status: LanguageSink,
    ) -> Result<Box<dyn LanguageServer>, IdeError> {
        let folder = PathBuf::from(root.display_path());
        Ok(Box::new(Workspace {
            root: root.clone(),
            status,
            // Asked once, as the workspace opens.
            applies: self
                .specs
                .iter()
                .map(|spec| spec.applies.is_none_or(|applies| applies(&folder)))
                .collect(),
            specs: Arc::clone(&self.specs),
            find_node: Arc::clone(&self.find_node),
            slots: Mutex::new(vec![Slot::Idle; self.specs.len()]),
        }))
    }
}

#[derive(Clone)]
enum Slot {
    Idle,
    Running(Arc<Session>),
    /// It couldn't start: not tried again until the servers are restarted.
    Failed,
}

/// One workspace's servers, each started the first time it's needed.
struct Workspace {
    root: WorkspaceRoot,
    status: LanguageSink,
    specs: Arc<[ServerSpec]>,
    /// Whether each server serves this workspace.
    applies: Vec<bool>,
    find_node: FindNode,
    slots: Mutex<Vec<Slot>>,
}

impl Workspace {
    /// The first server that serves this workspace and handles `path`.
    fn index_for(&self, path: &RelPath) -> Option<usize> {
        self.specs.iter().enumerate().position(|(index, spec)| {
            self.applies.get(index).copied().unwrap_or(false) && (spec.language)(path).is_some()
        })
    }

    /// The server for `path`, started now if it's the first of its files.
    fn server(&self, path: &RelPath) -> Option<Arc<Session>> {
        let index = self.index_for(path)?;
        let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        match slots.get(index)? {
            Slot::Running(session) => return Some(Arc::clone(session)),
            Slot::Failed => return None,
            Slot::Idle => {}
        }
        let spec = &self.specs[index];
        let started = self.program(spec).and_then(|(program, args)| {
            start_session(spec, &program, &args, &self.root, &self.status).ok()
        });
        let slot = match started {
            Some(session) => Slot::Running(Arc::new(session)),
            None => Slot::Failed,
        };
        let found = match &slot {
            Slot::Running(session) => Some(Arc::clone(session)),
            _ => None,
        };
        slots[index] = slot;
        found
    }

    /// The server for `path`, if it's running.
    fn running(&self, path: &RelPath) -> Option<Arc<Session>> {
        let index = self.index_for(path)?;
        match self
            .slots
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(index)?
        {
            Slot::Running(session) => Some(Arc::clone(session)),
            _ => None,
        }
    }

    /// `spec`'s arguments, with the workspace's folder in them.
    fn args(&self, spec: &ServerSpec) -> Vec<String> {
        let folder = self.root.display_path();
        spec.args
            .iter()
            .map(|arg| arg.replace(WORKSPACE_ARG, &folder))
            .collect()
    }

    /// What to run for `spec`, or why it can't be (said to the person).
    fn program(&self, spec: &ServerSpec) -> Option<(PathBuf, Vec<String>)> {
        let missing = |what: &Path| {
            (self.status)(
                spec.name,
                LanguageStatus::Failed(format!(
                    "the {} language server isn't installed with this app ({})",
                    spec.name,
                    what.display()
                )),
            );
            None
        };
        if let Some((files, why)) = spec.needs {
            let folder = PathBuf::from(self.root.display_path());
            if files.iter().any(|file| !folder.join(file).is_file()) {
                (self.status)(spec.name, LanguageStatus::Failed(why.to_owned()));
                return None;
            }
        }
        match &spec.program {
            Program::Native(program) if program.is_file() => {
                Some((program.clone(), self.args(spec)))
            }
            Program::Native(program) => missing(program),
            Program::Node(script) if !script.is_file() => missing(script),
            Program::Node(script) => {
                let Some(node) = (self.find_node)() else {
                    (self.status)(spec.name, LanguageStatus::NeedsNode);
                    return None;
                };
                let mut args = vec![script.to_string_lossy().into_owned()];
                args.extend(self.args(spec));
                Some((node, args))
            }
        }
    }
}

impl LanguageServer for Workspace {
    fn open(&self, path: &RelPath, text: &str) -> Result<(), IdeError> {
        self.server(path).map_or(Ok(()), |s| s.open(path, text))
    }
    fn change(&self, path: &RelPath, text: &str) -> Result<(), IdeError> {
        self.server(path).map_or(Ok(()), |s| s.change(path, text))
    }
    fn close(&self, path: &RelPath) -> Result<(), IdeError> {
        // Never started just to be told a file closed.
        self.running(path).map_or(Ok(()), |s| s.close(path))
    }
    fn problems(&self, path: &RelPath) -> Result<Vec<Problem>, IdeError> {
        self.server(path)
            .map_or(Ok(Vec::new()), |s| s.problems(path))
    }
    fn completions(&self, path: &RelPath, at: TextPosition) -> Result<Completions, IdeError> {
        self.server(path)
            .map_or(Ok(Completions::default()), |s| s.completions(path, at))
    }
    fn hover(&self, path: &RelPath, at: TextPosition) -> Result<Option<HoverText>, IdeError> {
        self.server(path).map_or(Ok(None), |s| s.hover(path, at))
    }
    fn definition(&self, path: &RelPath, at: TextPosition) -> Result<Vec<Definition>, IdeError> {
        self.server(path)
            .map_or(Ok(Vec::new()), |s| s.definition(path, at))
    }
    fn resolve_completion(
        &self,
        path: &RelPath,
        resolve: &str,
    ) -> Result<ResolvedCompletion, IdeError> {
        self.server(path)
            .map_or(Ok(ResolvedCompletion::default()), |s| {
                s.resolve_completion(path, resolve)
            })
    }
    fn fixes(&self, path: &RelPath, range: TextRange) -> Result<Vec<CodeAction>, IdeError> {
        self.server(path)
            .map_or(Ok(Vec::new()), |s| s.fixes(path, range))
    }
    fn source_action(
        &self,
        path: &RelPath,
        action: SourceAction,
    ) -> Result<Option<Vec<TextEdit>>, IdeError> {
        self.server(path)
            .map_or(Ok(None), |s| s.source_action(path, action))
    }
}
