#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify_debouncer_full::DebouncedEvent;
use notify_debouncer_full::notify::event::{CreateKind, DataChange, Flag, ModifyKind, RemoveKind};

use super::*;

fn root() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"\\?\C:\ws")
    } else {
        PathBuf::from("/ws")
    }
}

fn at(rel: &str) -> PathBuf {
    let mut path = root();
    for segment in rel.split('/') {
        path.push(segment);
    }
    path
}

fn p(rel: &str) -> RelPath {
    RelPath::parse(rel).unwrap()
}

fn event(kind: EventKind, paths: &[PathBuf]) -> Event {
    let mut event = Event::new(kind);
    for path in paths {
        event = event.add_path(path.clone());
    }
    event
}

#[test]
fn maps_os_events_to_changes() {
    let temp = format!("src/{TEMP_FILE_PREFIX}1234");
    let outside = if cfg!(windows) {
        PathBuf::from(r"\\?\C:\elsewhere\a.ts")
    } else {
        PathBuf::from("/elsewhere/a.ts")
    };
    let cases: Vec<(Event, Vec<FsChange>)> = vec![
        (
            event(EventKind::Create(CreateKind::File), &[at("src/a.ts")]),
            vec![FsChange::Created(p("src/a.ts"))],
        ),
        (
            event(
                EventKind::Modify(ModifyKind::Data(DataChange::Content)),
                &[at("a.ts")],
            ),
            vec![FsChange::Modified(p("a.ts"))],
        ),
        (
            event(EventKind::Remove(RemoveKind::Folder), &[at("old")]),
            vec![FsChange::Removed(p("old"))],
        ),
        (
            event(
                EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
                &[at("a.ts"), at("b.ts")],
            ),
            vec![FsChange::Renamed {
                from: p("a.ts"),
                to: p("b.ts"),
            }],
        ),
        (
            event(
                EventKind::Modify(ModifyKind::Name(RenameMode::From)),
                &[at("a.ts")],
            ),
            vec![FsChange::Removed(p("a.ts"))],
        ),
        (
            event(
                EventKind::Modify(ModifyKind::Name(RenameMode::To)),
                &[at("b.ts")],
            ),
            vec![FsChange::Created(p("b.ts"))],
        ),
        // Our own save: the temporary file is renamed over the target.
        (
            event(
                EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
                &[at(&temp), at("src/a.ts")],
            ),
            vec![FsChange::Created(p("src/a.ts"))],
        ),
        (
            event(EventKind::Create(CreateKind::File), &[at(&temp)]),
            vec![],
        ),
        (
            event(EventKind::Create(CreateKind::File), &[outside]),
            vec![],
        ),
        (
            event(
                EventKind::Access(notify_debouncer_full::notify::event::AccessKind::Any),
                &[at("a.ts")],
            ),
            vec![],
        ),
    ];
    for (input, expected) in cases {
        assert_eq!(to_changes(&root(), &input), expected, "{input:?}");
    }
}

#[test]
fn a_batch_with_a_rescan_notice_asks_for_a_rescan() {
    let rescan = Event::new(EventKind::Other).set_flag(Flag::Rescan);
    let modified = event(
        EventKind::Modify(ModifyKind::Data(DataChange::Any)),
        &[at("a.ts")],
    );
    let batch = vec![
        DebouncedEvent::new(rescan, Instant::now()),
        DebouncedEvent::new(modified, Instant::now()),
    ];
    assert_eq!(
        to_watch_event(&root(), Ok(batch)),
        Some(WatchEvent::Changed {
            changes: vec![FsChange::Modified(p("a.ts"))],
            rescan: true,
        })
    );
}

#[test]
fn a_batch_of_only_ignored_events_reports_nothing() {
    let temp = at(&format!("{TEMP_FILE_PREFIX}1"));
    let batch = vec![DebouncedEvent::new(
        event(EventKind::Create(CreateKind::File), &[temp]),
        Instant::now(),
    )];
    assert_eq!(to_watch_event(&root(), Ok(batch)), None);
}

// ---------- On the real file system ----------

fn workspace() -> (tempfile::TempDir, WorkspaceRoot) {
    let dir = tempfile::tempdir().unwrap();
    let canonical = dir.path().canonicalize().unwrap();
    let root = WorkspaceRoot::from_canonical(canonical).unwrap();
    (dir, root)
}

/// Collects events until one mentions `path`, or the time runs out.
fn wait_for(events: &mpsc::Receiver<WatchEvent>, path: &RelPath) -> Vec<FsChange> {
    wait_until(events, &format!("a change to {path}"), |change| {
        change.path() == path
    })
}

/// Collects events until `change` itself arrives. (FSEvents can deliver a
/// late `Modified` for a path before the change a test is waiting for.)
fn wait_for_change(events: &mpsc::Receiver<WatchEvent>, change: &FsChange) -> Vec<FsChange> {
    wait_until(events, &format!("{change:?}"), |seen| seen == change)
}

fn wait_until(
    events: &mpsc::Receiver<WatchEvent>,
    what: &str,
    found: impl Fn(&FsChange) -> bool,
) -> Vec<FsChange> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut seen = Vec::new();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match events.recv_timeout(left) {
            Ok(WatchEvent::Changed { changes, .. }) => {
                let done = changes.iter().any(&found);
                seen.extend(changes);
                if done {
                    return seen;
                }
            }
            Ok(WatchEvent::Failed { message }) => panic!("watch failed: {message}"),
            Err(_) => break,
        }
    }
    panic!("no {what} within 10 s; saw {seen:?}");
}

fn start_watch(root: &WorkspaceRoot) -> (WatchGuard, mpsc::Receiver<WatchEvent>) {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let guard = LocalWatcher::with_debounce(Duration::from_millis(50)).watch(
        root,
        Arc::new(move |event| {
            let _ = tx.lock().unwrap().send(event);
        }),
    );
    // The watch starts on another thread; give it time to be in place.
    std::thread::sleep(Duration::from_millis(300));
    (guard, rx)
}

#[test]
fn reports_files_created_changed_and_removed_on_disk() {
    let (_dir, root) = workspace();
    let (_guard, events) = start_watch(&root);
    let file = root.path().join("a.txt");

    std::fs::write(&file, "one").unwrap();
    wait_for(&events, &p("a.txt"));

    // inotify watches a new folder only once it has seen it created, so a
    // file written into it at once can be missed (the folder's creation is
    // what the explorer needs). Wait for the folder first.
    std::fs::create_dir(root.path().join("src")).unwrap();
    wait_for(&events, &p("src"));
    std::fs::write(root.path().join("src").join("b.txt"), "b").unwrap();
    wait_for(&events, &p("src/b.txt"));

    std::fs::remove_file(&file).unwrap();
    wait_for_change(&events, &FsChange::Removed(p("a.txt")));
}

#[test]
fn an_atomic_write_reports_the_target_and_never_its_temporary_file() {
    let (_dir, root) = workspace();
    std::fs::write(root.path().join("a.txt"), "one").unwrap();
    let (_guard, events) = start_watch(&root);

    // What ide-fs does: write a temporary file beside the target, then rename it over.
    let temp = root.path().join(format!("{TEMP_FILE_PREFIX}test"));
    std::fs::write(&temp, "two").unwrap();
    std::fs::rename(&temp, root.path().join("a.txt")).unwrap();

    let seen = wait_for(&events, &p("a.txt"));
    // Anything still queued arrives within a few debounce periods.
    std::thread::sleep(Duration::from_millis(300));
    let rest: Vec<_> = events.try_iter().collect();
    let all = format!("{seen:?} {rest:?}");
    assert!(!all.contains(TEMP_FILE_PREFIX), "{all}");
}

#[test]
fn dropping_the_guard_stops_the_watch() {
    let (_dir, root) = workspace();
    let (guard, events) = start_watch(&root);
    drop(guard);
    std::fs::write(root.path().join("late.txt"), "x").unwrap();
    std::thread::sleep(Duration::from_millis(500));
    let late: Vec<_> = events.try_iter().collect();
    assert!(late.is_empty(), "{late:?}");
}
