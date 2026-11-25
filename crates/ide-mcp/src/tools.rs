//! The tools agents call. Each runs on tokio's blocking pool (the ports do
//! IO), is reported to the app, and fails with a sentence the agent can act on.

use std::fmt::Write as _;
use std::sync::Arc;

use ide_core::agent::{AgentCall, AgentSink, AgentWorkbench, ToolSpec, WorkspaceTools};
use ide_core::fs::{NodeKind, ReadOutcome, is_git_internal};
use ide_core::git::{DiffSide, DiffTexts, GitChange, GitState};
use ide_core::{
    CancelFlag, GitClient, IdeError, RelPath, TextQuery, WorkspaceFs, WorkspaceRoot,
    WorkspaceSearch,
};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::diff::unified_diff;

/// The most paths `find_files` lists, and lines `search_text` shows.
const MAX_FILES: usize = 1000;
const DEFAULT_FILES: usize = 200;
const MAX_MATCH_LINES: usize = 1000;
const DEFAULT_MATCH_LINES: usize = 200;

/// What the tools work with.
pub struct Ports {
    pub fs: Arc<dyn WorkspaceFs>,
    pub search: Arc<dyn WorkspaceSearch>,
    pub git: Arc<dyn GitClient>,
    pub workbench: Arc<dyn AgentWorkbench>,
    pub sink: AgentSink,
    pub allow_edits: bool,
}

/// The MCP server's handler: a fresh clone answers each request.
#[derive(Clone)]
pub struct Tools {
    ports: Arc<Ports>,
    tool_router: ToolRouter<Self>,
}

impl Tools {
    pub fn new(ports: Ports) -> Self {
        let mut tool_router = Self::tool_router();
        // Without edits allowed, agents aren't told there's a way to write.
        if !ports.allow_edits {
            tool_router.remove_route("write_file");
        }
        Self {
            ports: Arc::new(ports),
            tool_router,
        }
    }

    /// Runs `work` on the open workspace, off the server's thread, and
    /// reports the call.
    async fn run(
        &self,
        tool: &'static str,
        target: Option<String>,
        work: impl FnOnce(&Ports, &WorkspaceRoot) -> Result<String, String> + Send + 'static,
    ) -> Result<String, String> {
        let ports = Arc::clone(&self.ports);
        let result = tokio::task::spawn_blocking(move || run_now(&ports, work))
            .await
            .unwrap_or_else(|_| Err("the IDE failed while doing this".to_owned()));
        (self.ports.sink)(AgentCall {
            tool: tool.to_owned(),
            target,
            error: result.as_ref().err().cloned(),
        });
        result
    }
}

/// Runs `work` on the open workspace, on this thread.
fn run_now(
    ports: &Ports,
    work: impl FnOnce(&Ports, &WorkspaceRoot) -> Result<String, String>,
) -> Result<String, String> {
    let root = ports
        .workbench
        .root()
        .ok_or_else(|| "no folder is open in the IDE".to_owned())?;
    // A bare repository (or another git directory) opened as the folder.
    if looks_like_git_dir(root.path()) {
        return Err("the open folder is a git directory: agents can't use it".to_owned());
    }
    work(ports, &root)
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DirectoryArgs {
    /// A folder in the workspace, relative to its root ("src/lib"); the root when left out.
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadArgs {
    /// A file in the workspace, relative to its root ("src/main.ts").
    pub path: String,
    /// The first line to return, from 1.
    #[serde(default)]
    pub start_line: Option<usize>,
    /// The last line to return, included.
    #[serde(default)]
    pub end_line: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FindArgs {
    /// Letters the path must contain in order, in any case ("srcmain" finds
    /// "src/main.ts"). Every file when left out.
    #[serde(default)]
    pub query: Option<String>,
    /// The most paths to return (200 when left out, at most 1000).
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchArgs {
    /// What to find.
    pub pattern: String,
    /// `pattern` is a regular expression (Rust's syntax).
    #[serde(default)]
    pub is_regex: bool,
    #[serde(default)]
    pub match_case: bool,
    #[serde(default)]
    pub whole_word: bool,
    /// Globs of the files to search ("src/**", "*.ts"); all when empty.
    #[serde(default)]
    pub include: Vec<String>,
    /// Globs of files to skip.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// The most matching lines to return (200 when left out, at most 1000).
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DiffArgs {
    /// A changed file, relative to the workspace root.
    pub path: String,
    /// The staged change (index against HEAD) instead of the unstaged one
    /// (working tree against the index).
    #[serde(default)]
    pub staged: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WriteArgs {
    /// A file in the workspace, relative to its root. Its folder must exist.
    pub path: String,
    /// The file's whole new text.
    pub text: String,
}

#[tool_router]
impl Tools {
    #[tool(
        description = "The folder open in the IDE: its name and where it is. Every other tool works in it, with paths relative to it.",
        annotations(read_only_hint = true)
    )]
    async fn workspace_info(&self) -> Result<String, String> {
        self.run("workspace_info", None, workspace_info).await
    }

    #[tool(
        description = "What a folder in the workspace holds, one entry per line; folders end with '/'. .git isn't listed.",
        annotations(read_only_hint = true)
    )]
    async fn list_directory(
        &self,
        Parameters(args): Parameters<DirectoryArgs>,
    ) -> Result<String, String> {
        let target = args.path.clone();
        self.run("list_directory", target, move |ports, root| {
            list_directory(ports, root, args)
        })
        .await
    }

    #[tool(
        description = "A text file's contents, from the disk (unsaved changes in the IDE aren't included). Optionally only some lines.",
        annotations(read_only_hint = true)
    )]
    async fn read_file(&self, Parameters(args): Parameters<ReadArgs>) -> Result<String, String> {
        let target = Some(args.path.clone());
        self.run("read_file", target, move |ports, root| {
            read_file(ports, root, args)
        })
        .await
    }

    #[tool(
        description = "Files in the workspace whose paths match, one per line. Ignored files (.gitignore) and .git aren't included.",
        annotations(read_only_hint = true)
    )]
    async fn find_files(&self, Parameters(args): Parameters<FindArgs>) -> Result<String, String> {
        let target = args.query.clone();
        self.run("find_files", target, move |ports, root| {
            find_files(ports, root, args)
        })
        .await
    }

    #[tool(
        description = "Lines in the workspace's files that match, as 'path:line:column: text'. Ignored files (.gitignore) and .git aren't searched.",
        annotations(read_only_hint = true)
    )]
    async fn search_text(
        &self,
        Parameters(args): Parameters<SearchArgs>,
    ) -> Result<String, String> {
        let target = Some(args.pattern.clone());
        self.run("search_text", target, move |ports, root| {
            search_text(ports, root, args)
        })
        .await
    }

    #[tool(
        description = "The git branch and the changed files, as 'XY path' where X is the staged change and Y the unstaged one (M modified, A added, D deleted, R renamed, C copied, T type changed, U conflicted, ? untracked).",
        annotations(read_only_hint = true)
    )]
    async fn git_status(&self) -> Result<String, String> {
        self.run("git_status", None, git_status).await
    }

    #[tool(
        description = "A changed file's diff, unified: the unstaged change by default, or the staged one.",
        annotations(read_only_hint = true)
    )]
    async fn git_diff(&self, Parameters(args): Parameters<DiffArgs>) -> Result<String, String> {
        let target = Some(args.path.clone());
        self.run("git_diff", target, move |ports, root| {
            git_diff(ports, root, args)
        })
        .await
    }

    #[tool(
        description = "Replaces a file's whole text, or creates the file in an existing folder. The IDE shows the change; an editor with unsaved changes to the file keeps them and says the file changed on disk.",
        annotations(destructive_hint = true, read_only_hint = false)
    )]
    async fn write_file(&self, Parameters(args): Parameters<WriteArgs>) -> Result<String, String> {
        let target = Some(args.path.clone());
        self.run("write_file", target, move |ports, root| {
            write_file(ports, root, args)
        })
        .await
    }
}

fn workspace_info(_: &Ports, root: &WorkspaceRoot) -> Result<String, String> {
    Ok(format!(
        "name: {}\npath: {}",
        root.name(),
        root.display_path()
    ))
}

fn list_directory(
    ports: &Ports,
    root: &WorkspaceRoot,
    args: DirectoryArgs,
) -> Result<String, String> {
    let path = match args.path.as_deref() {
        Some(path) => workspace_relative(root, path)?,
        None => RelPath::root(),
    };
    refuse_git(root, &path, "read")?;
    let listing = ports.fs.list_dir(root, &path).map_err(say)?;
    // .git isn't shown: its config can hold credentials, and git's
    // own tools (git_status, git_diff) say what agents need.
    let entries: Vec<_> = listing
        .entries
        .iter()
        .filter(|entry| !is_git_internal(&entry.path))
        .collect();
    let mut out = String::new();
    for entry in &entries {
        match entry.kind {
            NodeKind::Directory => {
                let _ = writeln!(out, "{}/", entry.name);
            }
            _ => match entry.size {
                Some(size) => {
                    let _ = writeln!(out, "{} ({size} bytes)", entry.name);
                }
                None => {
                    let _ = writeln!(out, "{}", entry.name);
                }
            },
        }
    }
    if entries.is_empty() {
        out.push_str("(empty)\n");
    }
    if listing.truncated {
        out.push_str("(more entries not listed)\n");
    }
    Ok(out)
}

fn read_file(ports: &Ports, root: &WorkspaceRoot, args: ReadArgs) -> Result<String, String> {
    let path = workspace_relative(root, &args.path)?;
    refuse_git(root, &path, "read")?;
    let limits = ports.workbench.read_limits();
    match ports.fs.read_file(root, &path, limits).map_err(say)? {
        ReadOutcome::Text { text, .. } => Ok(lines(&text, args.start_line, args.end_line)),
        ReadOutcome::TooLarge { limit, .. } => Err(format!(
            "the file is larger than {limit} bytes, the most the IDE opens"
        )),
        ReadOutcome::Binary { .. } => Err("the file isn't text".to_owned()),
        ReadOutcome::NotUtf8 { .. } => Err("the file isn't UTF-8 text".to_owned()),
    }
}

fn find_files(ports: &Ports, root: &WorkspaceRoot, args: FindArgs) -> Result<String, String> {
    let list = ports
        .search
        .list_files(root, &CancelFlag::new())
        .map_err(say)?;
    let limit = args.limit.unwrap_or(DEFAULT_FILES).clamp(1, MAX_FILES);
    let query = args.query.unwrap_or_default().to_lowercase();
    let mut matched = list
        .files
        .iter()
        .filter(|path| in_order(&path.as_str().to_lowercase(), &query));
    let mut out = String::new();
    for path in matched.by_ref().take(limit) {
        let _ = writeln!(out, "{}", path.as_str());
    }
    if matched.next().is_some() || list.truncated {
        out.push_str("(more files not listed: narrow the query)\n");
    }
    if out.is_empty() {
        out.push_str("(no files match)\n");
    }
    Ok(out)
}

fn search_text(ports: &Ports, root: &WorkspaceRoot, args: SearchArgs) -> Result<String, String> {
    let limit = args
        .limit
        .unwrap_or(DEFAULT_MATCH_LINES)
        .clamp(1, MAX_MATCH_LINES);
    let query = TextQuery {
        pattern: args.pattern,
        is_regex: args.is_regex,
        match_case: args.match_case,
        whole_word: args.whole_word,
        include: args.include,
        exclude: args.exclude,
        max_file_bytes: ports.workbench.read_limits().refuse_above,
    };
    let outcome = ports
        .search
        .search_text(root, &query, &CancelFlag::new())
        .map_err(say)?;
    let mut out = String::new();
    let mut shown = 0;
    let mut more = outcome.truncated;
    'files: for file in &outcome.files {
        for line in &file.lines {
            if shown == limit {
                more = true;
                break 'files;
            }
            let _ = writeln!(
                out,
                "{}:{}:{}: {}",
                file.path.as_str(),
                line.line,
                line.column,
                line.preview
            );
            shown += 1;
        }
    }
    if shown == 0 {
        out.push_str("(no matches)\n");
    }
    if more {
        out.push_str("(more matches not listed: narrow the search)\n");
    }
    Ok(out)
}

fn git_status(ports: &Ports, root: &WorkspaceRoot) -> Result<String, String> {
    match ports.git.state(root).map_err(say)? {
        GitState::Unavailable => Err("git isn't installed".to_owned()),
        GitState::NotARepository => Ok("not a git repository\n".to_owned()),
        GitState::Repository(status) => {
            let head = &status.head;
            let mut out = match (&head.branch, &head.commit) {
                (Some(branch), _) => format!("on branch {branch}"),
                (None, Some(commit)) => format!("detached at {commit}"),
                (None, None) => "no commits yet".to_owned(),
            };
            if let Some(upstream) = &head.upstream {
                let _ = write!(
                    out,
                    " (tracking {upstream}: {} ahead, {} behind)",
                    head.ahead, head.behind
                );
            }
            out.push('\n');
            for file in &status.files {
                // Untracked is "??", as git writes it.
                let (x, y) = match (file.staged, file.unstaged) {
                    (_, Some(GitChange::Untracked)) | (Some(GitChange::Untracked), _) => ('?', '?'),
                    (staged, unstaged) => (code(staged), code(unstaged)),
                };
                let _ = write!(out, "{x}{y} {}", file.path.as_str());
                if let Some(from) = &file.from {
                    let _ = write!(out, " (from {})", from.as_str());
                }
                out.push('\n');
            }
            if status.files.is_empty() {
                out.push_str("nothing changed\n");
            }
            if status.truncated {
                out.push_str("(more changed files not listed)\n");
            }
            Ok(out)
        }
    }
}

fn git_diff(ports: &Ports, root: &WorkspaceRoot, args: DiffArgs) -> Result<String, String> {
    let path = workspace_relative(root, &args.path)?;
    refuse_git(root, &path, "read")?;
    let side = if args.staged {
        DiffSide::Staged
    } else {
        DiffSide::Unstaged
    };
    let limit = ports.workbench.read_limits().refuse_above;
    match ports
        .git
        .diff_texts(root, &path, side, limit)
        .map_err(say)?
    {
        DiffTexts::Text { original, modified } => {
            let diff = unified_diff(path.as_str(), &original, &modified);
            Ok(if diff.is_empty() {
                "no changes\n".to_owned()
            } else {
                diff
            })
        }
        DiffTexts::Binary => Ok("binary files differ\n".to_owned()),
        DiffTexts::TooLarge { limit } => Err(format!(
            "a version is larger than {limit} bytes, the most the IDE opens"
        )),
    }
}

fn write_file(ports: &Ports, root: &WorkspaceRoot, args: WriteArgs) -> Result<String, String> {
    if !ports.allow_edits {
        return Err("edits aren't allowed: the person can allow them in the IDE".to_owned());
    }
    let path = workspace_relative(root, &args.path)?;
    refuse_git(root, &path, "written")?;
    let stamp = ports.fs.write_file(root, &path, &args.text).map_err(say)?;
    Ok(format!("wrote {} ({} bytes)", path.as_str(), stamp.size))
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Tools {
    fn get_info(&self) -> ServerConfig {
        let edits = if self.ports.allow_edits {
            "write_file replaces a file's text."
        } else {
            "The workspace is read only: the person hasn't allowed edits."
        };
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("webide", env!("CARGO_PKG_VERSION")))
            .with_instructions(format!(
                "Tools for the folder open in the webide IDE. Paths are relative to \
                 its root, with '/' between names; workspace_info says where it is. \
                 Files are read from the disk. {edits}"
            ))
    }
}

/// The same tools for the IDE's own assistant: the read tools, called by
/// name, and writes the person accepted. Nothing is reported as an agent's call.
pub struct Toolbox {
    ports: Ports,
}

impl Toolbox {
    /// `ports` with edits refused: a write comes only through `write_text`.
    pub fn new(mut ports: Ports) -> Self {
        ports.allow_edits = false;
        Self { ports }
    }
}

fn parse<T: serde::de::DeserializeOwned>(arguments: &str) -> Result<T, String> {
    let arguments = if arguments.trim().is_empty() {
        "{}"
    } else {
        arguments
    };
    serde_json::from_str(arguments).map_err(|error| format!("the arguments don't fit: {error}"))
}

impl WorkspaceTools for Toolbox {
    fn read_tools(&self) -> Vec<ToolSpec> {
        Tools::tool_router()
            .list_all()
            .into_iter()
            .filter(|tool| {
                tool.annotations
                    .as_ref()
                    .and_then(|annotations| annotations.read_only_hint)
                    == Some(true)
            })
            .map(|tool| ToolSpec {
                name: tool.name.into_owned(),
                description: tool
                    .description
                    .map(|text| text.into_owned())
                    .unwrap_or_default(),
                input_schema: serde_json::to_string(&*tool.input_schema)
                    .unwrap_or_else(|_| r#"{"type":"object"}"#.to_owned()),
            })
            .collect()
    }

    fn call(&self, name: &str, arguments: &str) -> Result<String, String> {
        let ports = &self.ports;
        match name {
            "workspace_info" => run_now(ports, workspace_info),
            "list_directory" => {
                let args = parse(arguments)?;
                run_now(ports, |ports, root| list_directory(ports, root, args))
            }
            "read_file" => {
                let args = parse(arguments)?;
                run_now(ports, |ports, root| read_file(ports, root, args))
            }
            "find_files" => {
                let args = parse(arguments)?;
                run_now(ports, |ports, root| find_files(ports, root, args))
            }
            "search_text" => {
                let args = parse(arguments)?;
                run_now(ports, |ports, root| search_text(ports, root, args))
            }
            "git_status" => run_now(ports, git_status),
            "git_diff" => {
                let args = parse(arguments)?;
                run_now(ports, |ports, root| git_diff(ports, root, args))
            }
            other => Err(format!("there's no tool called {other}")),
        }
    }

    fn read_text(&self, path: &str) -> Result<(String, Option<String>), String> {
        let mut found = (String::new(), None);
        run_now(&self.ports, |ports, root| {
            let path = workspace_relative(root, path)?;
            refuse_git(root, &path, "read")?;
            let limits = ports.workbench.read_limits();
            found.0 = path.as_str().to_owned();
            found.1 = match ports.fs.read_file(root, &path, limits) {
                Ok(ReadOutcome::Text { text, .. }) => Some(text),
                Ok(ReadOutcome::TooLarge { limit, .. }) => {
                    return Err(format!(
                        "the file is larger than {limit} bytes, the most the IDE opens"
                    ));
                }
                Ok(ReadOutcome::Binary { .. } | ReadOutcome::NotUtf8 { .. }) => {
                    return Err("the file isn't UTF-8 text".to_owned());
                }
                Err(IdeError::NotFound) => None,
                Err(error) => return Err(say(error)),
            };
            Ok(String::new())
        })?;
        Ok(found)
    }

    fn write_text(&self, path: &str, text: &str) -> Result<(), String> {
        run_now(&self.ports, |ports, root| {
            let path = workspace_relative(root, path)?;
            refuse_git(root, &path, "written")?;
            ports.fs.write_file(root, &path, text).map_err(say)?;
            Ok(String::new())
        })
        .map(|_| ())
    }
}

/// `input` as a workspace path: relative to the root ("src/a.ts", "./src",
/// "src\\a.ts"), or absolute inside it.
pub fn workspace_relative(root: &WorkspaceRoot, input: &str) -> Result<RelPath, String> {
    let mut path = input.trim().replace('\\', "/");
    let base = root.display_path().replace('\\', "/");
    let base = base.trim_end_matches('/');
    if let Some(rest) = strip_base(&path, base) {
        path = rest.to_owned();
    }
    let path = path.trim_start_matches("./").trim_end_matches('/');
    if path.is_empty() || path == "." {
        return Ok(RelPath::root());
    }
    RelPath::parse(path).map_err(|error| {
        if path.starts_with('/') || path.chars().nth(1) == Some(':') {
            format!("{input} isn't in the workspace ({})", root.display_path())
        } else {
            format!("{input} isn't a usable path: {error}")
        }
    })
}

/// What follows `base` in `path`, when `path` is in it. Case doesn't matter on Windows.
fn strip_base<'a>(path: &'a str, base: &str) -> Option<&'a str> {
    let head = path.get(..base.len())?;
    let same = if cfg!(windows) {
        head.eq_ignore_ascii_case(base)
    } else {
        head == base
    };
    if !same {
        return None;
    }
    let rest = &path[base.len()..];
    if rest.is_empty() {
        Some("")
    } else {
        rest.strip_prefix('/')
    }
}

/// Lines `start..=end` (from 1) of `text`, or all of it.
fn lines(text: &str, start: Option<usize>, end: Option<usize>) -> String {
    if start.is_none() && end.is_none() {
        return text.to_owned();
    }
    let first = start.unwrap_or(1).max(1);
    let last = end.unwrap_or(usize::MAX);
    text.split_inclusive('\n')
        .skip(first - 1)
        .take(last.saturating_sub(first - 1))
        .collect()
}

/// Whether `query`'s characters appear in `text` in order.
fn in_order(text: &str, query: &str) -> bool {
    let mut letters = text.chars();
    query
        .chars()
        .filter(|c| !c.is_whitespace())
        .all(|wanted| letters.any(|c| c == wanted))
}

fn code(change: Option<GitChange>) -> char {
    match change {
        None => ' ',
        Some(GitChange::Modified) => 'M',
        Some(GitChange::Added) => 'A',
        Some(GitChange::Deleted) => 'D',
        Some(GitChange::Renamed) => 'R',
        Some(GitChange::Copied) => 'C',
        Some(GitChange::TypeChanged) => 'T',
        Some(GitChange::Untracked) => '?',
        Some(GitChange::Conflicted) => 'U',
    }
}

/// Agents don't list, read or write a git directory: its config can hold
/// credentials (a remote's URL with a token in it), and a hook written there
/// would run on the next commit. `path` is checked as written, then as what
/// it names on disk (after symlinks, case and Windows short names such as
/// `GIT~1`): every folder from the root down to it is refused when it's named
/// `.git`, or is a git directory by another name (a bare repository, or where
/// a `gitdir:` file points). A path that doesn't exist yet is checked by the
/// part of it that does. Paths outside the workspace are left to the ports, which refuse them.
pub(crate) fn refuse_git(root: &WorkspaceRoot, path: &RelPath, verb: &str) -> Result<(), String> {
    let refused = || {
        Err(format!(
            "files in a git directory can't be {verb}: git_status and git_diff say what changed"
        ))
    };
    if is_git_internal(path) {
        return refused();
    }
    let base = root.path();
    let native = path.to_native(base);
    // The nearest part of it that exists, after symlinks and aliases.
    let Some(resolved) = native
        .ancestors()
        .find_map(|ancestor| std::fs::canonicalize(ancestor).ok())
    else {
        return Ok(());
    };
    let Ok(rest) = resolved.strip_prefix(base) else {
        return Ok(());
    };
    let mut dir = base.to_path_buf();
    for component in rest.components() {
        dir.push(component);
        if component.as_os_str().eq_ignore_ascii_case(".git") || looks_like_git_dir(&dir) {
            return refused();
        }
    }
    Ok(())
}

/// Whether `dir` is a git directory: `HEAD`, `objects` and `refs`, as git
/// itself decides.
pub(crate) fn looks_like_git_dir(dir: &std::path::Path) -> bool {
    dir.join("HEAD").is_file() && dir.join("objects").is_dir() && dir.join("refs").is_dir()
}

fn say(error: IdeError) -> String {
    error.to_string()
}
