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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub label: String,
    pub kind: CompletionKind,
    /// A type or signature, shown beside it.
    pub detail: Option<String>,
    /// What's inserted, when it isn't the label.
    pub insert: Option<String>,
    /// The order the server suggests.
    pub sort: Option<String>,
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

/// How a workspace's language server is doing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LanguageStatus {
    Starting,
    Ready,
    /// It couldn't start, or stopped; why.
    Failed(String),
}

/// Where a language server's status goes, from its own threads.
pub type LanguageSink = Arc<dyn Fn(LanguageStatus) + Send + Sync>;

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
}

/// Starts language servers.
pub trait LanguageServers: Send + Sync {
    /// Whether a server here handles this file (by its extension).
    fn handles(&self, path: &RelPath) -> bool;
    /// Starts one for the workspace at `root`; its status goes to `status`.
    fn start(
        &self,
        root: &WorkspaceRoot,
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
