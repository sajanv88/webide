//! Watches a workspace for changes and reports them as [`ide_core::watch`] events.
//!
//! One recursive OS watch per workspace (ReadDirectoryChangesW on Windows,
//! FSEvents on macOS, inotify on Linux), debounced so a burst of writes
//! arrives as one batch. Paths become workspace-relative [`RelPath`]s; the
//! temporary files of our own atomic writes are left out, so a save reports
//! only the file it replaced.

use std::path::{Component, Path};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use ide_core::fs::TEMP_FILE_PREFIX;
use ide_core::watch::coalesce;
use ide_core::{FsChange, FsWatcher, RelPath, WatchEvent, WatchGuard, WatchSink, WorkspaceRoot};
use notify_debouncer_full::notify::event::{ModifyKind, RenameMode};
use notify_debouncer_full::notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, NoCache, new_debouncer_opt};

/// How long the debouncer waits for a burst of events to settle.
pub const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(200);

/// [`FsWatcher`] on the local disk.
#[derive(Debug, Clone, Copy)]
pub struct LocalWatcher {
    debounce: Duration,
}

impl Default for LocalWatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalWatcher {
    pub fn new() -> Self {
        Self {
            debounce: DEFAULT_DEBOUNCE,
        }
    }

    pub fn with_debounce(debounce: Duration) -> Self {
        Self { debounce }
    }
}

type ActiveDebouncer = Debouncer<RecommendedWatcher, NoCache>;

/// Shared by the guard and the thread that starts the watch.
#[derive(Default)]
struct Slot {
    stopped: AtomicBool,
    debouncer: Mutex<Option<ActiveDebouncer>>,
}

/// Dropping it stops the watch, or stops it from starting.
struct Stopper(Arc<Slot>);

impl Drop for Stopper {
    fn drop(&mut self) {
        self.0.stopped.store(true, Ordering::Release);
        let debouncer = self
            .0
            .debouncer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(debouncer) = debouncer {
            debouncer.stop_nonblocking();
        }
    }
}

impl FsWatcher for LocalWatcher {
    fn watch(&self, root: &WorkspaceRoot, sink: WatchSink) -> WatchGuard {
        let slot = Arc::new(Slot::default());
        let root = root.path().to_path_buf();
        let debounce = self.debounce;
        let thread_slot = Arc::clone(&slot);
        // Starting a recursive watch can take a while (inotify adds one watch
        // per folder), so it runs on its own thread and opening a workspace
        // doesn't wait for it.
        let started = std::thread::Builder::new()
            .name("ide-watcher start".to_owned())
            .spawn(move || start(&root, debounce, sink, &thread_slot));
        if let Err(error) = started {
            // Nothing else can report it: the sink moved into the closure that failed to spawn.
            eprintln!("ide-watcher: couldn't start the watch thread: {error}");
        }
        WatchGuard::new(Stopper(slot))
    }
}

fn start(root: &Path, debounce: Duration, sink: WatchSink, slot: &Arc<Slot>) {
    let handler_slot = Arc::clone(slot);
    let handler_root = root.to_path_buf();
    let handler_sink = Arc::clone(&sink);
    let handler = move |result: DebounceEventResult| {
        if handler_slot.stopped.load(Ordering::Acquire) {
            return;
        }
        if let Some(event) = to_watch_event(&handler_root, result) {
            handler_sink(event);
        }
    };
    let config = Config::default().with_follow_symlinks(false);
    let started = new_debouncer_opt::<_, RecommendedWatcher, NoCache>(
        debounce,
        None,
        handler,
        NoCache::new(),
        config,
    )
    .and_then(|mut debouncer| {
        debouncer.watch(root, RecursiveMode::Recursive)?;
        Ok(debouncer)
    });
    match started {
        Ok(debouncer) => {
            let mut held = slot
                .debouncer
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if slot.stopped.load(Ordering::Acquire) {
                drop(held);
                debouncer.stop_nonblocking();
            } else {
                *held = Some(debouncer);
            }
        }
        Err(error) => {
            if !slot.stopped.load(Ordering::Acquire) {
                sink(WatchEvent::Failed {
                    message: error.to_string(),
                });
            }
        }
    }
}

/// One debounced batch as a [`WatchEvent`]; `None` when nothing in it matters.
fn to_watch_event(root: &Path, result: DebounceEventResult) -> Option<WatchEvent> {
    match result {
        Ok(events) => {
            let mut rescan = false;
            let mut changes = Vec::new();
            for event in &events {
                if event.need_rescan() {
                    rescan = true;
                } else {
                    changes.extend(to_changes(root, event));
                }
            }
            match coalesce(changes) {
                Some(changes) if changes.is_empty() && !rescan => None,
                Some(changes) => Some(WatchEvent::Changed { changes, rescan }),
                None => Some(WatchEvent::Changed {
                    changes: Vec::new(),
                    rescan: true,
                }),
            }
        }
        Err(errors) => Some(WatchEvent::Failed {
            message: errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; "),
        }),
    }
}

/// What one OS event means for the workspace.
fn to_changes(root: &Path, event: &Event) -> Vec<FsChange> {
    let paths = || event.paths.iter().filter_map(|path| relative(root, path));
    match event.kind {
        EventKind::Access(_) => Vec::new(),
        EventKind::Create(_) | EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
            paths().map(FsChange::Created).collect()
        }
        EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
            paths().map(FsChange::Removed).collect()
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => match event.paths.as_slice() {
            [from, to] => match (relative(root, from), relative(root, to)) {
                (Some(from), Some(to)) => vec![FsChange::Renamed { from, to }],
                // Our own save: the temporary file took the target's place.
                (None, Some(to)) => vec![FsChange::Created(to)],
                (Some(from), None) => vec![FsChange::Removed(from)],
                (None, None) => Vec::new(),
            },
            _ => paths().map(FsChange::Modified).collect(),
        },
        EventKind::Modify(_) | EventKind::Any | EventKind::Other => {
            paths().map(FsChange::Modified).collect()
        }
    }
}

/// `path` relative to `root`, unless it's outside it, has a name that can't be
/// a [`RelPath`], or is one of our own temporary files.
fn relative(root: &Path, path: &Path) -> Option<RelPath> {
    let rest = path.strip_prefix(root).ok()?;
    let mut rel = RelPath::root();
    for component in rest.components() {
        let Component::Normal(name) = component else {
            return None;
        };
        rel = rel.join(name.to_str()?).ok()?;
    }
    if rel
        .file_name()
        .is_some_and(|name| name.starts_with(TEMP_FILE_PREFIX))
    {
        return None;
    }
    Some(rel)
}

#[cfg(test)]
mod tests;
