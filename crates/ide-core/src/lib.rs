//! Pure domain types, errors and port traits for the IDE.
//!
//! No Tauri, no IO, no async runtime. Feature crates implement the traits in
//! [`ports`]; `ide-ipc` consumes them.

pub mod agent;
pub mod assistant;
pub mod debug;
pub mod error;
pub mod fs;
pub mod git;
pub mod language;
pub mod path;
pub mod ports;
pub mod search;
pub mod settings;
pub mod terminal;
pub mod watch;
pub mod workspace;

pub use error::IdeError;
pub use path::{NameRules, PathError, RelPath};
pub use ports::{FsWatcher, GitClient, WorkspaceFs, WorkspaceSearch, WorkspaceStore};
pub use search::{
    CancelFlag, FileList, FileMatches, LineMatch, Span, TextQuery, TextSearchOutcome,
};
pub use settings::{EditorSettings, LineHighlight, Settings, SettingsPatch, Theme, Whitespace};
pub use terminal::{TerminalEvent, TerminalHost, TerminalSession, TerminalSink, TerminalSize};
pub use watch::{FsChange, WatchEvent, WatchGuard, WatchSink};
pub use workspace::{RecentWorkspace, WorkspaceRoot};
