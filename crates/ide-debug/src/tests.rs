use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ide_core::debug::{DebugAdapters, DebugEvent, FrameSource};
use ide_core::{RelPath, WorkspaceRoot};

use crate::{NodeDebugger, find_node, workspace_path};

#[test]
fn debugger_paths_map_to_the_workspace() {
    let rel = |root: &str, path: &str| workspace_path(root, path).map(|p| p.as_str().to_owned());
    assert_eq!(
        rel(r"C:\Users\me\app", r"c:\Users\me\app\src\a.js"),
        Some("src/a.js".to_owned())
    );
    assert_eq!(rel(r"C:\Users\me\app", r"c:\Users\me\app-2\a.js"), None);
    assert_eq!(
        rel("/home/me/app", "/home/me/app/a.js"),
        Some("a.js".to_owned())
    );
    assert_eq!(rel("/home/me/app", "node:internal/main"), None);
}

#[test]
fn it_debugs_scripts_by_extension() {
    let debugger = NodeDebugger::new(PathBuf::from("x"));
    for (path, ok) in [
        ("a.js", true),
        ("a.mjs", true),
        ("a.ts", true),
        ("a.json", false),
        ("README", false),
    ] {
        assert_eq!(
            debugger.handles(&RelPath::parse(path).unwrap()),
            ok,
            "{path}"
        );
    }
}

/// js-debug as `pnpm build` stages it.
fn staged_server() -> Option<PathBuf> {
    let server = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/desktop/src-tauri/resources/js-debug/src/dapDebugServer.js");
    server.is_file().then_some(server)
}

fn next_event(
    events: &mpsc::Receiver<DebugEvent>,
    until: impl Fn(&DebugEvent) -> bool,
    seen: &mut Vec<DebugEvent>,
) -> Option<DebugEvent> {
    let deadline = Instant::now() + Duration::from_secs(30);
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match events.recv_timeout(left) {
            Ok(event) if until(&event) => return Some(event),
            Ok(event) => seen.push(event),
            Err(_) => return None,
        }
    }
    None
}

#[test]
fn a_node_program_stops_at_a_breakpoint_and_runs_to_its_end() {
    let (Some(server), Some(_)) = (staged_server(), find_node()) else {
        eprintln!("skipping: needs node and the staged js-debug (pnpm build)");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("app.js"),
        "const a = 1;\nconst b = a + 1;\nconsole.log('sum', b);\n",
    )
    .unwrap();
    let root = WorkspaceRoot::from_canonical(std::fs::canonicalize(dir.path()).unwrap()).unwrap();
    let (tx, events) = mpsc::channel();
    let tx = Mutex::new(tx);
    let app = RelPath::parse("app.js").unwrap();
    let session = NodeDebugger::new(server)
        .launch(
            &root,
            &app,
            &[(app.clone(), vec![2])],
            Arc::new(move |event| {
                let _ = tx.lock().unwrap().send(event);
            }),
        )
        .unwrap();

    let mut seen = Vec::new();
    let Some(DebugEvent::Stopped {
        thread_id, reason, ..
    }) = next_event(
        &events,
        |e| matches!(e, DebugEvent::Stopped { .. }),
        &mut seen,
    )
    else {
        panic!("it didn't stop: {seen:?}");
    };
    assert_eq!(reason, "breakpoint");

    let stack = session.stack(thread_id).unwrap();
    let top = &stack[0];
    assert_eq!(top.line, 2);
    assert_eq!(top.source, Some(FrameSource::Workspace(app.clone())));
    let scopes = session.scopes(top.id).unwrap();
    let local = scopes.iter().find(|scope| scope.name == "Local").unwrap();
    let variables = session.variables(local.reference).unwrap();
    assert!(
        variables.iter().any(|v| v.name == "a" && v.value == "1"),
        "{variables:?}"
    );
    assert_eq!(session.evaluate("a + 41", Some(top.id)).unwrap(), "42");

    session.resume(thread_id).unwrap();
    let mut output = Vec::new();
    assert!(
        next_event(
            &events,
            |e| matches!(e, DebugEvent::Terminated),
            &mut output
        )
        .is_some(),
        "it didn't end: {output:?}"
    );
    let printed: String = output
        .iter()
        .filter_map(|event| match event {
            DebugEvent::Output { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(printed.contains("sum 2"), "{printed:?}");
    drop(session);
}
