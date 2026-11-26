//! Language features (problems, completion, hover, go to definition) from a
//! language server, in the IDE's own terms. Positions are 0-based lines and
//! UTF-16 columns, as the editor and LSP count them.

use std::sync::Arc;

use crate::IdeError;
use crate::path::RelPath;
use crate::workspace::WorkspaceRoot;

/// A place in a text: 0-based line, and column in UTF-16 units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TextPosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextRange {
    pub start: TextPosition,
    pub end: TextPosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
    Hint,
}

/// A problem in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub range: TextRange,
    pub severity: Severity,
    pub message: String,
    /// What found it: `ts`.
    pub source: Option<String>,
    /// Its code: `2322`.
    pub code: Option<String>,
}

/// What a completion is, for its icon and its word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Text,
    Method,
    Function,
    Constructor,
    Field,
    Variable,
    Class,
    Interface,
    Module,
    Property,
    Keyword,
    Snippet,
    Constant,
    Enum,
    EnumMember,
    TypeParameter,
    File,
    Folder,
    Other,
}

/// A change to a file's text: `range` is replaced by `new_text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub range: TextRange,
    pub new_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub label: String,
    pub kind: CompletionKind,
    /// A type or signature, shown beside it.
    pub detail: Option<String>,
    /// What's inserted, when it isn't the label.
    pub insert: Option<String>,
    /// Where what's inserted starts, when not where the word typed does
    /// (a JSON key replaces its opening quote too).
    pub replace_from: Option<TextPosition>,
    /// The order the server suggests.
    pub sort: Option<String>,
    /// Where it comes from, when it isn't in scope yet: the module an import
    /// would name (`react`).
    pub source: Option<String>,
    /// Other changes made with it, such as an import.
    pub edits: Vec<TextEdit>,
    /// Ask [`LanguageServer::resolve_completion`] with this for what else
    /// picking it does (the import it adds); `None` when there's nothing more.
    pub resolve: Option<String>,
}

/// What else picking a completion does.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResolvedCompletion {
    /// What it does, said: `Add import from "react"`.
    pub detail: Option<String>,
    /// The changes made with it, such as the import.
    pub edits: Vec<TextEdit>,
}

/// A change the server offers for a problem: "Add import from \"react\"".
/// Its edits are all to the file it was asked about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeAction {
    pub title: String,
    /// The one the server thinks is meant.
    pub preferred: bool,
    pub edits: Vec<TextEdit>,
}

/// A change to a whole file the server makes on request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceAction {
    /// Sorts the imports and removes the unused ones.
    OrganizeImports,
    RemoveUnusedImports,
    SortImports,
    /// Every fix the server is sure of.
    FixAll,
}

impl SourceAction {
    /// LSP's code action kind for it.
    pub fn kind(self) -> &'static str {
        match self {
            Self::OrganizeImports => "source.organizeImports",
            Self::RemoveUnusedImports => "source.removeUnusedImports",
            Self::SortImports => "source.sortImports",
            Self::FixAll => "source.fixAll",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Completions {
    pub items: Vec<Completion>,
    /// More would come with more typed: ask again.
    pub incomplete: bool,
}

/// What a hover shows: Markdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoverText {
    pub markdown: String,
}

/// Where something is defined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Definition {
    /// In the workspace: it can be opened.
    Workspace { path: RelPath, range: TextRange },
    /// Outside it (a library's types, the language's own): only named.
    Outside { path: String, range: TextRange },
}

/// How one of a workspace's language servers is doing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LanguageStatus {
    Starting,
    Ready,
    /// It runs on Node.js, which isn't installed (or wasn't found).
    NeedsNode,
    /// It couldn't start, or stopped; why.
    Failed(String),
    /// It would run the folder's own code, and the person hasn't trusted
    /// the folder: it stays off.
    Restricted,
}

/// Where the language servers' statuses go, from their own threads: the
/// server's name ("TypeScript", "JSON") and how it's doing.
pub type LanguageSink = Arc<dyn Fn(&str, LanguageStatus) + Send + Sync>;

/// One language server for one workspace. Dropping it stops the server.
pub trait LanguageServer: Send + Sync {
    /// The file is open in the editor with `text`.
    fn open(&self, path: &RelPath, text: &str) -> Result<(), IdeError>;
    /// Its text is now `text`.
    fn change(&self, path: &RelPath, text: &str) -> Result<(), IdeError>;
    fn close(&self, path: &RelPath) -> Result<(), IdeError>;
    fn problems(&self, path: &RelPath) -> Result<Vec<Problem>, IdeError>;
    fn completions(&self, path: &RelPath, at: TextPosition) -> Result<Completions, IdeError>;
    fn hover(&self, path: &RelPath, at: TextPosition) -> Result<Option<HoverText>, IdeError>;
    fn definition(&self, path: &RelPath, at: TextPosition) -> Result<Vec<Definition>, IdeError>;
    /// What else picking a completion does, by its [`Completion::resolve`].
    fn resolve_completion(
        &self,
        _path: &RelPath,
        _resolve: &str,
    ) -> Result<ResolvedCompletion, IdeError> {
        Ok(ResolvedCompletion::default())
    }
    /// The fixes for the problems in `range` (an import to add…).
    fn fixes(&self, _path: &RelPath, _range: TextRange) -> Result<Vec<CodeAction>, IdeError> {
        Ok(Vec::new())
    }
    /// The edits that make `action` on the file; `None` when the server
    /// doesn't offer it there, empty when there's nothing to change.
    fn source_action(
        &self,
        _path: &RelPath,
        _action: SourceAction,
    ) -> Result<Option<Vec<TextEdit>>, IdeError> {
        Ok(None)
    }
}

/// Starts language servers.
pub trait LanguageServers: Send + Sync {
    /// Whether a server here handles this file (by its extension).
    fn handles(&self, path: &RelPath) -> bool;
    /// Starts one for the workspace at `root`; its status goes to `status`.
    /// Unless `trusted`, a server that would run the folder's own code stays off.
    fn start(
        &self,
        root: &WorkspaceRoot,
        trusted: bool,
        status: LanguageSink,
    ) -> Result<Box<dyn LanguageServer>, IdeError>;
}

/// The LSP language id of a JavaScript or TypeScript file, by its extension.
pub fn script_language(path: &RelPath) -> Option<&'static str> {
    let name = path.file_name()?;
    let extension = name.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match extension.as_str() {
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "typescriptreact",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "javascriptreact",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_languages_by_extension() {
        let lang = |path: &str| script_language(&RelPath::parse(path).unwrap());
        assert_eq!(lang("src/a.ts"), Some("typescript"));
        assert_eq!(lang("src/A.TSX"), Some("typescriptreact"));
        assert_eq!(lang("x.mjs"), Some("javascript"));
        assert_eq!(lang("x.jsx"), Some("javascriptreact"));
        assert_eq!(lang("x.json"), None);
        assert_eq!(lang("Makefile"), None);
    }
}
