//! What each command does, as plain functions over [`IpcState`].
//! No Tauri here, so they're tested with fake ports.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use ide_core::workspace::display_path;
use ide_core::{
    FsWatcher, IdeError, RelPath, WatchEvent, WatchGuard, WatchSink, WorkspaceFs, WorkspaceRoot,
    WorkspaceStore,
};

use crate::dto::{
    AppInfoDto, BackendEventDto, DirListingDto, FileStampDto, OpenRecentArgs, ReadFileDto,
    RecentWorkspaceDto, SettingsDto, SettingsPatchDto, WorkspaceDto, WorkspacePathArgs,
    WriteFileArgs,
};
use crate::error::IpcError;

/// The implementations `ide-ipc` runs on, built by the composition root.
#[derive(Clone)]
pub struct Services {
    pub fs: Arc<dyn WorkspaceFs>,
    pub workspace: Arc<dyn WorkspaceStore>,
    pub watcher: Arc<dyn FsWatcher>,
}

impl std::fmt::Debug for Services {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Services").finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
struct OpenWorkspace {
    id: String,
    root: WorkspaceRoot,
}

/// Delivers an event to the webview, if one is listening.
pub type Emit = Arc<dyn Fn(BackendEventDto) + Send + Sync>;

/// Shared by every command.
pub struct IpcState {
    services: Services,
    app_info: AppInfoDto,
    open: RwLock<Option<OpenWorkspace>>,
    next_id: AtomicU64,
    emit: Emit,
    /// The watch on the open workspace; replacing it stops the old one.
    watch: Mutex<Option<WatchGuard>>,
}

impl std::fmt::Debug for IpcState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IpcState")
            .field("open", &self.open)
            .finish_non_exhaustive()
    }
}

impl IpcState {
    pub fn new(services: Services, app_info: AppInfoDto, emit: Emit) -> Self {
        Self {
            services,
            app_info,
            open: RwLock::new(None),
            next_id: AtomicU64::new(1),
            emit,
            watch: Mutex::new(None),
        }
    }

    /// Makes `root` the open workspace, under a new id, and watches it instead
    /// of the previous one.
    fn set_open(&self, root: WorkspaceRoot) -> WorkspaceDto {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed).to_string();
        let dto = WorkspaceDto {
            id: id.clone(),
            name: root.name().to_owned(),
            path: root.display_path(),
        };
        let mut watch = self.watch.lock().unwrap_or_else(PoisonError::into_inner);
        // Stop the old watch first, so its last events can't follow the new id.
        watch.take();
        *watch = Some(self.services.watcher.watch(&root, self.sink(id.clone())));
        *self.open.write().unwrap_or_else(PoisonError::into_inner) =
            Some(OpenWorkspace { id, root });
        dto
    }

    /// Where the watch of workspace `id` reports: to the webview, tagged with `id`.
    fn sink(&self, id: String) -> WatchSink {
        let emit = Arc::clone(&self.emit);
        Arc::new(move |event| {
            emit(match event {
                WatchEvent::Changed { changes, rescan } => BackendEventDto::FilesChanged {
                    workspace_id: id.clone(),
                    changes: changes.into_iter().map(Into::into).collect(),
                    rescan,
                },
                WatchEvent::Failed { message } => BackendEventDto::WatchFailed {
                    workspace_id: id.clone(),
                    message,
                },
            });
        })
    }

    fn current(&self) -> Option<OpenWorkspace> {
        self.open
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The root of the open workspace, if `workspace_id` is still the open one.
    fn root_for(&self, workspace_id: &str) -> Result<WorkspaceRoot, IdeError> {
        match self.current() {
            None => Err(IdeError::NoWorkspace),
            Some(open) if open.id == workspace_id => Ok(open.root),
            Some(_) => Err(IdeError::WorkspaceChanged),
        }
    }
}

// Every handler is `fn(&IpcState[, Args]) -> Result<Output, IpcError>`; the
// command list in `commands.rs` checks each against its declared types.

pub fn app_info(state: &IpcState) -> Result<AppInfoDto, IpcError> {
    Ok(state.app_info.clone())
}

/// Opens a folder the person chose in the native folder picker.
pub fn open_picked(state: &IpcState, path: &Path) -> Result<WorkspaceDto, IpcError> {
    let root = state.services.workspace.open(path)?;
    Ok(state.set_open(root))
}

/// Opens a folder from the recent list. The webview can't name any other
/// folder: a root is only ever chosen by the person, in the native picker.
pub fn open_recent_workspace(
    state: &IpcState,
    args: OpenRecentArgs,
) -> Result<WorkspaceDto, IpcError> {
    let recent = state.services.workspace.recent()?;
    let entry = recent
        .into_iter()
        .find(|entry| display_path(&entry.path) == args.path)
        .ok_or(IdeError::NotARecentWorkspace)?;
    let root = state.services.workspace.open(&entry.path)?;
    Ok(state.set_open(root))
}

pub fn recent_workspaces(state: &IpcState) -> Result<Vec<RecentWorkspaceDto>, IpcError> {
    Ok(state
        .services
        .workspace
        .recent()?
        .iter()
        .map(Into::into)
        .collect())
}

pub fn current_workspace(state: &IpcState) -> Result<Option<WorkspaceDto>, IpcError> {
    Ok(state.current().map(|open| WorkspaceDto {
        id: open.id,
        name: open.root.name().to_owned(),
        path: open.root.display_path(),
    }))
}

pub fn list_directory(
    state: &IpcState,
    args: WorkspacePathArgs,
) -> Result<DirListingDto, IpcError> {
    let path = RelPath::parse(&args.path)?;
    let root = state.root_for(&args.workspace_id)?;
    Ok(state.services.fs.list_dir(&root, &path)?.into())
}

pub fn read_file(state: &IpcState, args: WorkspacePathArgs) -> Result<ReadFileDto, IpcError> {
    let path = RelPath::parse(&args.path)?;
    let root = state.root_for(&args.workspace_id)?;
    let limits = state.services.workspace.settings()?.read_limits();
    let outcome = state.services.fs.read_file(&root, &path, limits)?;
    Ok(ReadFileDto::from_outcome(&path, outcome))
}

pub fn write_file(state: &IpcState, args: WriteFileArgs) -> Result<FileStampDto, IpcError> {
    let path = RelPath::parse(&args.path)?;
    let root = state.root_for(&args.workspace_id)?;
    Ok(state
        .services
        .fs
        .write_file(&root, &path, &args.text)?
        .into())
}

pub fn get_settings(state: &IpcState) -> Result<SettingsDto, IpcError> {
    Ok(state.services.workspace.settings()?.into())
}

pub fn update_settings(state: &IpcState, patch: SettingsPatchDto) -> Result<SettingsDto, IpcError> {
    Ok(state
        .services
        .workspace
        .update_settings(&patch.into())?
        .into())
}
