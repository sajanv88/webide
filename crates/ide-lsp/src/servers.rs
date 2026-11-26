//! Which servers there are, the files each one handles, and starting them:
//! each the first time a file of its kind is opened in a workspace.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use ide_core::language::StartOptions;
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
    /// It runs the folder's own code (its config, plugins or packages), so
    /// it's off until the person trusts the folder.
    pub needs_trust: bool,
    /// The one server for its files, or one beside it (a linter).
    pub role: Role,
    /// Whether it's on with these options; always when absent.
    pub switch: Option<fn(&StartOptions) -> bool>,
}

/// What a server is to the files it handles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The first one that handles a file answers everything about it.
    Primary,
    /// Beside the primary one: it's told about the file too, and its
    /// problems and fixes are added to the primary's (ESLint).
    Linter,
}

/// In a server's settings, the workspace folder's `file:` URI, and its name.
pub const WORKSPACE_URI_SETTING: &str = "{workspaceUri}";
pub const WORKSPACE_NAME_SETTING: &str = "{workspaceName}";

/// `settings` with the workspace's URI and name where they're asked for.
pub(crate) fn with_workspace(settings: &Value, uri: &str, name: &str) -> Value {
    match settings {
        Value::String(text) if text == WORKSPACE_URI_SETTING => Value::String(uri.to_owned()),
        Value::String(text) if text == WORKSPACE_NAME_SETTING => Value::String(name.to_owned()),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| with_workspace(item, uri, name))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), with_workspace(value, uri, name)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// In a server's `args`, the workspace's folder.
pub const WORKSPACE_ARG: &str = "{workspace}";

/// Whether the folder is an Angular project: `angular.json`, or
/// `@angular/core` in its `package.json`.
pub fn angular_workspace(folder: &Path) -> bool {
    let read = |name: &str| -> Option<Value> {
        serde_json::from_str(&std::fs::read_to_string(folder.join(name)).ok()?).ok()
    };
    // angular.json's own shape: its projects.
    let config = read("angular.json")
        .is_some_and(|config| config.get("projects").is_some_and(Value::is_object));
    // Or @angular/core among the package's dependencies.
    let package = read("package.json").is_some_and(|package| {
        ["dependencies", "devDependencies", "peerDependencies"]
            .iter()
            .any(|kind| {
                package
                    .get(kind)
                    .and_then(|deps| deps.get("@angular/core"))
                    .is_some()
            })
    });
    config || package
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

pub fn vue_language(path: &RelPath) -> Option<&'static str> {
    (extension(path)?.as_str() == "vue").then_some("vue")
}

pub fn svelte_language(path: &RelPath) -> Option<&'static str> {
    (extension(path)?.as_str() == "svelte").then_some("svelte")
}

/// The folder of the TypeScript bundled beside a server's `script`
/// (`node_modules/typescript/lib`), as a server's `tsdk` names it.
fn bundled_typescript(script: &Path) -> String {
    script
        .parent()
        .unwrap_or(script)
        .join("node_modules")
        .join("typescript")
        .join("lib")
        .to_string_lossy()
        .into_owned()
}

/// The files ESLint checks: scripts, and Vue and Svelte components.
pub fn eslint_language(path: &RelPath) -> Option<&'static str> {
    script_language(path).or_else(|| match extension(path)?.as_str() {
        "vue" => Some("vue"),
        "svelte" => Some("svelte"),
        _ => None,
    })
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
    /// `@angular/language-service` from the copies bundled beside it (in
    /// `node_modules` there), never the project's: opening a template runs
    /// nothing from the workspace, which it only reads. Listed before HTML,
    /// which then has the `.html` files of every other project.
    pub fn angular(script: PathBuf) -> Self {
        let script = plain_path(script);
        let bundled = script
            .parent()
            .unwrap_or(&script)
            .to_string_lossy()
            .into_owned();
        Self {
            name: "Angular",
            program: Program::Node(script),
            args: vec![
                "--stdio".to_owned(),
                "--tsProbeLocations".to_owned(),
                bundled.clone(),
                "--ngProbeLocations".to_owned(),
                bundled,
            ],
            language: html_language,
            initialization_options: Value::Null,
            settings: Value::Null,
            schemas: None,
            notifications: Vec::new(),
            applies: Some(angular_workspace),
            needs_trust: false,
            role: Role::Primary,
            switch: None,
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
            needs_trust: false,
            role: Role::Primary,
            switch: None,
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
            needs_trust: false,
            role: Role::Primary,
            switch: None,
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
            needs_trust: false,
            role: Role::Primary,
            switch: None,
        }
    }

    /// Vue's single-file components (`.vue`): Vue's server (`script`, 2.x)
    /// on Node, in its own TypeScript mode (`hybridMode` off), so it answers
    /// for the template, the script and the styles. Its TypeScript is the
    /// copy bundled beside it (`node_modules/typescript`). It reads the
    /// project's `tsconfig.json`, whose `vueCompilerOptions` can name plugins
    /// it loads, so it needs the folder trusted.
    pub fn vue(script: PathBuf) -> Self {
        let script = plain_path(script);
        let tsdk = bundled_typescript(&script);
        Self {
            name: "Vue",
            program: Program::Node(script),
            args: vec!["--stdio".to_owned()],
            language: vue_language,
            initialization_options: json!({
                "typescript": { "tsdk": tsdk },
                "vue": { "hybridMode": false },
            }),
            settings: Value::Null,
            schemas: None,
            notifications: Vec::new(),
            applies: None,
            needs_trust: true,
            role: Role::Primary,
            switch: None,
        }
    }

    /// Svelte components (`.svelte`): Svelte's server (`script`) on Node. It
    /// loads the project's own `svelte` compiler and runs its
    /// `svelte.config.js`, so it needs the folder trusted. Its TypeScript is
    /// the copy bundled beside it.
    pub fn svelte(script: PathBuf) -> Self {
        let script = plain_path(script);
        Self {
            name: "Svelte",
            program: Program::Node(script),
            args: vec!["--stdio".to_owned()],
            language: svelte_language,
            initialization_options: json!({
                "configuration": {
                    "svelte": { "plugin": { "svelte": { "format": { "enable": false } } } },
                },
            }),
            settings: json!({
                "svelte": { "plugin": { "svelte": { "format": { "enable": false } } } },
            }),
            schemas: None,
            notifications: Vec::new(),
            applies: None,
            needs_trust: true,
            role: Role::Primary,
            switch: None,
        }
    }

    /// ESLint, beside the file's own server, for JavaScript and TypeScript
    /// (and Vue and Svelte files, which ESLint can check with their
    /// plugins): VS Code's ESLint server, on Node. It loads the project's
    /// own `eslint` and its configuration, so it needs the folder trusted;
    /// and it's off unless the person turns it on. A project without ESLint
    /// gets nothing from it.
    pub fn eslint(script: PathBuf) -> Self {
        let script = plain_path(script);
        Self {
            name: "ESLint",
            program: Program::Node(script),
            args: vec!["--stdio".to_owned()],
            language: eslint_language,
            initialization_options: Value::Null,
            // What VS Code's ESLint extension sends: the server reads every
            // field, so none may be missing.
            settings: json!({
                "validate": "on",
                "packageManager": "npm",
                "useESLintClass": false,
                "experimental": { "useFlatConfig": false },
                "codeAction": {
                    "disableRuleComment": {
                        "enable": true,
                        "location": "separateLine",
                        "commentStyle": "line",
                    },
                    "showDocumentation": { "enable": false },
                },
                "codeActionOnSave": { "mode": "all" },
                "format": false,
                "quiet": false,
                "onIgnoredFiles": "off",
                "options": {},
                "rulesCustomizations": [],
                "run": "onType",
                "problems": { "shortenToSingleLine": false },
                "nodePath": null,
                "workspaceFolder": { "uri": WORKSPACE_URI_SETTING, "name": WORKSPACE_NAME_SETTING },
            }),
            schemas: None,
            notifications: Vec::new(),
            applies: None,
            needs_trust: true,
            role: Role::Linter,
            switch: Some(|options| options.eslint),
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
            needs_trust: false,
            role: Role::Primary,
            switch: None,
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
            needs_trust: false,
            role: Role::Primary,
            switch: None,
        }
    }
}

/// How a server answers the requests a server makes: its settings, and the
/// bundled schemas it asks for by address.
pub(crate) fn responder(spec: &ServerSpec, root: &WorkspaceRoot) -> OnRequest {
    let settings = with_workspace(&spec.settings, &to_uri(&root.display_path()), root.name());
    let schemas = spec.schemas.clone();
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
        options: StartOptions,
        status: LanguageSink,
    ) -> Result<Box<dyn LanguageServer>, IdeError> {
        let trusted = options.trusted;
        let folder = PathBuf::from(root.display_path());
        // Asked once, as the workspace opens.
        let applies: Vec<bool> = self
            .specs
            .iter()
            .map(|spec| {
                spec.switch.is_none_or(|on| on(&options))
                    && spec.applies.is_none_or(|applies| applies(&folder))
            })
            .collect();
        Ok(Box::new(Workspace {
            root: root.clone(),
            status,
            restricted: self
                .specs
                .iter()
                .zip(&applies)
                .map(|(spec, applies)| *applies && spec.needs_trust && !trusted)
                .map(AtomicBool::new)
                .collect(),
            applies: self
                .specs
                .iter()
                .zip(applies)
                .map(|(spec, applies)| applies && (trusted || !spec.needs_trust))
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
    /// The servers that would serve it but need the folder trusted; each is
    /// cleared once it has said so.
    restricted: Vec<AtomicBool>,
    find_node: FindNode,
    slots: Mutex<Vec<Slot>>,
}

impl Workspace {
    /// Whether server `index` serves this workspace and handles `path`.
    fn serves(&self, index: usize, path: &RelPath) -> bool {
        self.applies.get(index).copied().unwrap_or(false)
            && (self.specs[index].language)(path).is_some()
    }

    /// The first primary server that serves this workspace and handles
    /// `path`. A server that would have had it (a primary one before it, or
    /// a linter) but is off until the folder is trusted says so, once.
    fn index_for(&self, path: &RelPath) -> Option<usize> {
        let index = (0..self.specs.len())
            .find(|&index| self.specs[index].role == Role::Primary && self.serves(index, path));
        for (at, (spec, restricted)) in self.specs.iter().zip(&self.restricted).enumerate() {
            let would = spec.role == Role::Linter || index.is_none_or(|index| at < index);
            if would && (spec.language)(path).is_some() && restricted.swap(false, Ordering::Relaxed)
            {
                (self.status)(spec.name, LanguageStatus::Restricted);
            }
        }
        index
    }

    /// The linters that serve this workspace and handle `path`.
    fn linters_for(&self, path: &RelPath) -> Vec<usize> {
        (0..self.specs.len())
            .filter(|&index| self.specs[index].role == Role::Linter && self.serves(index, path))
            .collect()
    }

    /// The primary server for `path`, started now if it's the first of its files.
    fn server(&self, path: &RelPath) -> Option<Arc<Session>> {
        self.slot(self.index_for(path)?)
    }

    /// The linters for `path`, each started now if it's the first of its files.
    fn linters(&self, path: &RelPath) -> Vec<Arc<Session>> {
        self.linters_for(path)
            .into_iter()
            .filter_map(|index| self.slot(index))
            .collect()
    }

    /// Server `index`, started now if it isn't yet.
    fn slot(&self, index: usize) -> Option<Arc<Session>> {
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

    /// The servers for `path` that are running, the primary one first.
    fn running(&self, path: &RelPath) -> Vec<Arc<Session>> {
        let slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        self.index_for(path)
            .into_iter()
            .chain(self.linters_for(path))
            .filter_map(|index| match slots.get(index)? {
                Slot::Running(session) => Some(Arc::clone(session)),
                _ => None,
            })
            .collect()
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
        if let Some(server) = self.server(path) {
            server.open(path, text)?;
        }
        // A linter that fails is said in its status; the file's server goes on.
        for linter in self.linters(path) {
            let _ = linter.open(path, text);
        }
        Ok(())
    }
    fn change(&self, path: &RelPath, text: &str) -> Result<(), IdeError> {
        if let Some(server) = self.server(path) {
            server.change(path, text)?;
        }
        for linter in self.linters(path) {
            let _ = linter.change(path, text);
        }
        Ok(())
    }
    fn close(&self, path: &RelPath) -> Result<(), IdeError> {
        // Never started just to be told a file closed.
        for server in self.running(path) {
            server.close(path)?;
        }
        Ok(())
    }
    fn problems(&self, path: &RelPath) -> Result<Vec<Problem>, IdeError> {
        let mut found = match self.server(path) {
            Some(server) => server.problems(path)?,
            None => Vec::new(),
        };
        for linter in self.linters(path) {
            found.extend(linter.problems(path).unwrap_or_default());
        }
        Ok(found)
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
        let mut found = match self.server(path) {
            Some(server) => server.fixes(path, range)?,
            None => Vec::new(),
        };
        for linter in self.linters(path) {
            found.extend(linter.fixes(path, range).unwrap_or_default());
        }
        Ok(found)
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
