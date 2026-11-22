//! Pure domain types, errors and port traits for the IDE.
//!
//! No Tauri, no IO, no async runtime. Feature crates implement the traits in
//! [`ports`]; `ide-ipc` consumes them.

pub mod error;
pub mod fs;
pub mod path;
pub mod ports;
pub mod settings;
pub mod watch;
pub mod workspace;

pub use error::IdeError;
pub use path::{NameRules, PathError, RelPath};
pub use ports::{FsWatcher, WorkspaceFs, WorkspaceStore};
pub use settings::{Settings, SettingsPatch, Theme};
pub use watch::{FsChange, WatchEvent, WatchGuard, WatchSink};
pub use workspace::{RecentWorkspace, WorkspaceRoot};
