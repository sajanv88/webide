use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use ide_core::language::{
    CompletionKind, Definition, LanguageServers, LanguageStatus, Severity, SourceAction,
    TextPosition, TextRange,
};
use ide_core::{RelPath, WorkspaceRoot};
use serde_json::json;

use crate::{TypeScriptServers, completion, hover_markdown, problem};

#[test]
fn problems_completions_and_hovers_are_read_from_lsp_shapes() {
    let found = problem(&json!({
        "range": { "start": { "line": 0, "character": 6 }, "end": { "line": 0, "character": 7 } },
        "severity": 1, "code": 2322, "source": "ts",
        "message": "Type 'string' is not assignable to type 'number'.",
    }))
    .unwrap();
    assert_eq!(found.severity, Severity::Error);
    assert_eq!(found.code.as_deref(), Some("2322"));
    assert_eq!(found.range.start.character, 6);

    let item = completion(&json!({
        "label": "toFixed", "kind": 2, "detail": "(method) toFixed(): string",
        "textEdit": { "newText": "toFixed", "range": {} }, "sortText": "11",
    }))
    .unwrap();
    assert_eq!(item.kind, CompletionKind::Method);
    assert_eq!(item.insert.as_deref(), Some("toFixed"));

    assert_eq!(
        hover_markdown(&json!({ "kind": "markdown", "value": "```ts\nconst y: string\n```" })),
        "```ts\nconst y: string\n```"
    );
    assert_eq!(
        hover_markdown(&json!([{ "language": "ts", "value": "let a" }, "Docs."])),
        "```ts\nlet a\n```\n\nDocs."
    );
}

/// The TypeScript server pnpm installed for this platform, if it's there.
fn installed_server() -> Option<PathBuf> {
    let platform = match std::env::consts::OS {
        "windows" => "win32",
        "macos" => "darwin",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let store = repo.join("node_modules/.pnpm");
    let prefix = format!("@typescript+typescript-{platform}-{arch}@");
    let dir = std::fs::read_dir(&store)
        .ok()?
        .flatten()
        .find(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))?;
    let exe = if cfg!(windows) { "tsc.exe" } else { "tsc" };
    let program = dir.path().join(format!(
        "node_modules/@typescript/typescript-{platform}-{arch}/lib/{exe}"
    ));
    program.is_file().then_some(program)
}

#[test]
fn the_typescript_server_answers_about_a_real_project() {
    let Some(program) = installed_server() else {
        eprintln!("skipping: no TypeScript server installed (run pnpm install)");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let text = "const x: number = 'nope';\nexport const y = x.toFixed(2);\nconsole.log(y);\n";
    std::fs::write(dir.path().join("a.ts"), text).unwrap();
    let root = WorkspaceRoot::from_canonical(std::fs::canonicalize(dir.path()).unwrap()).unwrap();
    let statuses = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&statuses);
    let servers = TypeScriptServers::new(program);
    let a = RelPath::parse("a.ts").unwrap();
    assert!(servers.handles(&a));
    assert!(!servers.handles(&RelPath::parse("a.json").unwrap()));

    let server = servers
        .start(
            &root,
            Arc::new(move |status| seen.lock().unwrap().push(status)),
        )
        .unwrap();
    assert_eq!(
        *statuses.lock().unwrap(),
        [LanguageStatus::Starting, LanguageStatus::Ready]
    );
    server.open(&a, text).unwrap();

    let problems = server.problems(&a).unwrap();
    assert!(
        problems.iter().any(|p| p.code.as_deref() == Some("2322")),
        "{problems:?}"
    );

    // After `x.` on line 2.
    let completions = server
        .completions(
            &a,
            TextPosition {
                line: 1,
                character: 19,
            },
        )
        .unwrap();
    assert!(completions.items.iter().any(|item| item.label == "toFixed"));

    let hover = server
        .hover(
            &a,
            TextPosition {
                line: 1,
                character: 13,
            },
        )
        .unwrap()
        .expect("a hover");
    assert!(hover.markdown.contains("y"), "{}", hover.markdown);

    // `x` on line 2 is defined on line 1, in the workspace.
    let here = server
        .definition(
            &a,
            TextPosition {
                line: 1,
                character: 17,
            },
        )
        .unwrap();
    assert!(
        matches!(&here[..], [Definition::Workspace { path, range }] if path == &a && range.start.line == 0),
        "{here:?}"
    );
    // `console` is the language's own: outside.
    let lib = server
        .definition(
            &a,
            TextPosition {
                line: 2,
                character: 2,
            },
        )
        .unwrap();
    assert!(
        lib.iter()
            .all(|target| matches!(target, Definition::Outside { .. })),
        "{lib:?}"
    );

    // Fixing it clears the problem.
    let fixed = text.replace("'nope'", "1");
    server.change(&a, &fixed).unwrap();
    assert!(server.problems(&a).unwrap().is_empty());
    server.close(&a).unwrap();
    drop(server);
    // Asked to stop: not reported as a failure.
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(
        !statuses
            .lock()
            .unwrap()
            .iter()
            .any(|status| matches!(status, LanguageStatus::Failed(_)))
    );
}

#[test]
fn a_missing_server_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let root = WorkspaceRoot::from_canonical(std::fs::canonicalize(dir.path()).unwrap()).unwrap();
    let statuses = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&statuses);
    let servers = TypeScriptServers::new(dir.path().join("no-such-tsc"));
    assert!(
        servers
            .start(
                &root,
                Arc::new(move |status| seen.lock().unwrap().push(status))
            )
            .is_err()
    );
    assert!(matches!(
        statuses.lock().unwrap().as_slice(),
        [LanguageStatus::Failed(_)]
    ));
}

#[test]
fn imports_come_with_completions_fixes_and_organizing() {
    let Some(program) = installed_server() else {
        eprintln!("skipping: no TypeScript server installed (run pnpm install)");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("lib.ts"),
        "export function helper(): number {\n  return 1;\n}\nexport const unused = 2;\n",
    )
    .unwrap();
    // A project, so lib.ts is in the same program as a.ts.
    std::fs::write(
        dir.path().join("tsconfig.json"),
        r#"{ "compilerOptions": { "strict": true }, "include": ["*.ts"] }"#,
    )
    .unwrap();
    std::fs::write(dir.path().join("a.ts"), "").unwrap();
    let root = WorkspaceRoot::from_canonical(std::fs::canonicalize(dir.path()).unwrap()).unwrap();
    let server = TypeScriptServers::new(program)
        .start(&root, Arc::new(|_| {}))
        .unwrap();
    let a = RelPath::parse("a.ts").unwrap();
    let at = |line, character| TextPosition { line, character };

    // A name from another module: picking it adds the import.
    let typing = "export const n = help\n";
    server.open(&a, typing).unwrap();
    let completions = server.completions(&a, at(0, 21)).unwrap();
    let helper = completions
        .items
        .iter()
        .find(|item| item.label == "helper")
        .expect("helper is offered");
    assert_eq!(helper.source.as_deref(), Some("./lib"), "{helper:?}");
    let token = helper.resolve.as_deref().expect("it can be resolved");
    let resolved = server.resolve_completion(&a, token).unwrap();
    assert!(
        resolved
            .edits
            .iter()
            .any(|edit| edit.new_text.contains("import { helper } from \"./lib\"")),
        "{resolved:?}"
    );
    // A token from an older list is refused.
    server.completions(&a, at(0, 21)).unwrap();
    assert!(server.resolve_completion(&a, token).is_err());

    // Used without its import: the fix adds it.
    let missing = "export const n = helper();\n";
    server.change(&a, missing).unwrap();
    let problems = server.problems(&a).unwrap();
    let problem = problems
        .iter()
        .find(|p| p.code.as_deref() == Some("2304"))
        .expect("cannot find name");
    let fixes = server.fixes(&a, problem.range).unwrap();
    assert!(
        fixes.iter().any(|fix| fix.title.contains("import")
            && fix.edits.iter().any(|edit| edit.new_text.contains("./lib"))),
        "{fixes:?}"
    );
    // Nowhere near a problem: nothing to fix.
    assert!(
        server
            .fixes(
                &a,
                TextRange {
                    start: at(1, 0),
                    end: at(1, 0)
                }
            )
            .unwrap()
            .is_empty()
    );

    // An unused import is removed by organizing them.
    let unused = "import { helper, unused } from \"./lib\";\nexport const n = helper();\n";
    server.change(&a, unused).unwrap();
    let edits = server
        .source_action(&a, SourceAction::OrganizeImports)
        .unwrap()
        .expect("organize imports is offered");
    assert!(
        edits
            .iter()
            .any(|edit| edit.new_text.contains("import { helper } from \"./lib\"")),
        "{edits:?}"
    );
}
