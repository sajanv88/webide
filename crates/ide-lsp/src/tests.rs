use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use ide_core::language::{
    CompletionKind, Definition, LanguageServers, LanguageStatus, Severity, SourceAction,
    TextPosition, TextRange,
};
use ide_core::{RelPath, WorkspaceRoot};
use serde_json::json;

use crate::{
    Program, ServerSpec, Servers, completion, hover_markdown, json_language, problem,
    schema_content_for_test, yaml_language,
};

/// The TypeScript server alone, with no Node to find.
fn typescript(program: PathBuf) -> Servers {
    Servers::new(vec![ServerSpec::typescript(program)], Arc::new(|| None))
}

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
    let servers = typescript(program);
    let a = RelPath::parse("a.ts").unwrap();
    assert!(servers.handles(&a));
    assert!(!servers.handles(&RelPath::parse("a.json").unwrap()));

    let server = servers
        .start(
            &root,
            Arc::new(move |name: &str, status| {
                assert_eq!(name, "TypeScript");
                seen.lock().unwrap().push(status);
            }),
        )
        .unwrap();
    // Started with its first file.
    assert!(statuses.lock().unwrap().is_empty());
    server.open(&a, text).unwrap();
    assert_eq!(
        *statuses.lock().unwrap(),
        [LanguageStatus::Starting, LanguageStatus::Ready]
    );

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
    let servers = typescript(dir.path().join("no-such-tsc"));
    let server = servers
        .start(
            &root,
            Arc::new(move |_: &str, status| seen.lock().unwrap().push(status)),
        )
        .unwrap();
    let a = RelPath::parse("a.ts").unwrap();
    server.open(&a, "x").unwrap();
    assert!(server.problems(&a).unwrap().is_empty());
    // Said once: it isn't tried again for every file.
    assert!(matches!(
        statuses.lock().unwrap().as_slice(),
        [LanguageStatus::Failed(_)]
    ));
}

#[test]
fn a_server_on_node_asks_for_node_when_there_is_none() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("json-server.cjs");
    std::fs::write(&script, "").unwrap();
    let root = WorkspaceRoot::from_canonical(std::fs::canonicalize(dir.path()).unwrap()).unwrap();
    let statuses = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&statuses);
    let servers = Servers::new(
        vec![ServerSpec::json(script, dir.path().join("schemas"))],
        Arc::new(|| None),
    );
    let package = RelPath::parse("package.json").unwrap();
    assert!(servers.handles(&package));
    assert!(!servers.handles(&RelPath::parse("a.ts").unwrap()));
    let server = servers
        .start(
            &root,
            Arc::new(move |name: &str, status| {
                seen.lock().unwrap().push((name.to_owned(), status))
            }),
        )
        .unwrap();
    server.open(&package, "{}").unwrap();
    assert_eq!(
        *statuses.lock().unwrap(),
        [("JSON".to_owned(), LanguageStatus::NeedsNode)]
    );
    assert!(matches!(
        ServerSpec::json(PathBuf::new(), PathBuf::new()).program,
        Program::Node(_)
    ));
}

#[test]
fn files_go_to_their_language() {
    let id = |f: crate::LanguageId, path: &str| f(&RelPath::parse(path).unwrap());
    assert_eq!(id(json_language, "package.json"), Some("json"));
    assert_eq!(id(json_language, "tsconfig.app.json"), Some("jsonc"));
    assert_eq!(id(json_language, ".vscode/settings.json"), Some("jsonc"));
    assert_eq!(id(json_language, "a.jsonc"), Some("jsonc"));
    assert_eq!(id(json_language, "a.ts"), None);
    assert_eq!(id(yaml_language, ".github/workflows/ci.YML"), Some("yaml"));
    assert_eq!(id(crate::css_language, "a.scss"), Some("scss"));
    assert_eq!(id(crate::html_language, "index.htm"), Some("html"));
}

#[test]
fn only_bundled_schemas_are_given_by_their_address() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("package.json"), "{\"title\":\"p\"}").unwrap();
    let content = |uri: serde_json::Value| schema_content_for_test(Some(dir.path()), Some(&uri));
    assert_eq!(
        content(json!("https://json.schemastore.org/package.json")).as_deref(),
        Some("{\"title\":\"p\"}")
    );
    assert!(content(json!(["https://json.schemastore.org/package"])).is_some());
    assert!(content(json!("https://json.schemastore.org/other.json")).is_none());
    // Nothing outside the folder, and nothing from anywhere else.
    assert!(content(json!("https://json.schemastore.org/../secret.json")).is_none());
    assert!(content(json!("https://json.schemastore.org/a/b.json")).is_none());
    assert!(content(json!("https://example.com/package.json")).is_none());
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
    let server = typescript(program)
        .start(&root, Arc::new(|_: &str, _| {}))
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

/// `node` on the PATH, for the tests that run the bundled servers.
fn node_on_path() -> Option<PathBuf> {
    let name = if cfg!(windows) { "node.exe" } else { "node" };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// The servers stage-language-servers.mjs bundled, if it has run.
fn staged_servers() -> Option<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/desktop/src-tauri/resources/language-servers");
    // A plain absolute path, as the app's resource folder is (no `..`, no `\\?\`).
    let dir = std::fs::canonicalize(dir).ok()?;
    let plain = dir.to_string_lossy().trim_start_matches(r"\\?\").to_owned();
    let dir = PathBuf::from(plain);
    dir.join("json-server.cjs").is_file().then_some(dir)
}

#[test]
fn the_bundled_servers_check_json_yaml_and_css() {
    let (Some(node), Some(staged)) = (node_on_path(), staged_servers()) else {
        eprintln!("skipping: no node, or the language servers aren't staged (run pnpm build)");
        return;
    };
    let schemas = staged.join("schemas");
    let servers = Servers::new(
        vec![
            ServerSpec::json(staged.join("json-server.cjs"), schemas.clone()),
            ServerSpec::css(staged.join("css-server.cjs")),
            ServerSpec::yaml(staged.join("yaml-server.mjs"), schemas),
        ],
        Arc::new(move || Some(node.clone())),
    );
    // Not a dot-folder: the YAML server's patterns (`**/.github/…`) skip those.
    let dir = tempfile::Builder::new().prefix("webide").tempdir().unwrap();
    let root = WorkspaceRoot::from_canonical(std::fs::canonicalize(dir.path()).unwrap()).unwrap();
    let statuses = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&statuses);
    let server = servers
        .start(
            &root,
            Arc::new(move |name: &str, status| {
                seen.lock().unwrap().push(format!("{name} {status:?}"))
            }),
        )
        .unwrap();
    let at = |line, character| TextPosition { line, character };

    // package.json, against its bundled schema: "version" must be a string.
    let package = RelPath::parse("package.json").unwrap();
    server
        .open(&package, "{\n  \"name\": \"demo\",\n  \"version\": 1\n}\n")
        .unwrap();
    let problems = server.problems(&package).unwrap();
    assert!(
        problems
            .iter()
            .any(|p| p.range.start.line == 2 && p.message.to_lowercase().contains("string")),
        "{problems:?}"
    );
    let keys = server.completions(&package, at(1, 3)).unwrap();
    assert!(!keys.items.is_empty(), "package.json keys are offered");

    // A GitHub workflow, against its schema: the server sends its problems unasked.
    std::fs::create_dir_all(dir.path().join(".github/workflows")).unwrap();
    let workflow = RelPath::parse(".github/workflows/ci.yml").unwrap();
    server
        .open(
            &workflow,
            "on: push\njobs:\n  build:\n    runs-on: ubuntu-latest\n    stepz: []\n",
        )
        .unwrap();
    let problems = server.problems(&workflow).unwrap();
    assert!(!problems.is_empty(), "the unknown key is a problem");

    // CSS: an unknown property is warned about.
    let style = RelPath::parse("a.css").unwrap();
    server.open(&style, "a { colr: red; }\n").unwrap();
    let problems = server.problems(&style).unwrap();
    assert!(
        problems.iter().any(|p| p.message.contains("colr")),
        "{problems:?}"
    );

    // A schema a file names for itself, at an address: never fetched, by
    // either server. (Here a listener on this machine, which hears nothing.)
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = format!("http://{}/schema.json", listener.local_addr().unwrap());
    let own = RelPath::parse("own.yml").unwrap();
    server
        .open(
            &own,
            &format!("# yaml-language-server: $schema={address}\nkey: value\n"),
        )
        .unwrap();
    server.problems(&own).unwrap();
    let named = RelPath::parse("named.json").unwrap();
    server
        .open(
            &named,
            &format!("{{ \"$schema\": \"{address}\", \"a\": 1 }}"),
        )
        .unwrap();
    server.problems(&named).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert!(
        listener.accept().is_err(),
        "a server fetched a schema itself"
    );

    let statuses = statuses.lock().unwrap().clone();
    for name in ["JSON", "YAML", "CSS"] {
        assert!(statuses.contains(&format!("{name} Ready")), "{statuses:?}");
    }
}

#[test]
fn snippets_become_plain_text() {
    let item = |insert: &str| {
        completion(&json!({ "label": "x", "insertText": insert, "insertTextFormat": 2 }))
            .unwrap()
            .insert
            .unwrap()
    };
    assert_eq!(item("\"scripts\": {$1}"), "\"scripts\": {}");
    assert_eq!(item("\"name\": \"${1:demo}\"$0"), "\"name\": \"demo\"");
    assert_eq!(item("${1:outer ${2:inner}} end"), "outer inner end");
    assert_eq!(item("type: ${1|module,commonjs|}"), "type: module");
    assert_eq!(item(r"cost: \$5 ${TM_FILENAME}"), "cost: $5 ");
    // Plain text stays as it is, dollars and all.
    let plain = completion(&json!({ "label": "x", "insertText": "a $1 b" })).unwrap();
    assert_eq!(plain.insert.as_deref(), Some("a $1 b"));
    assert_eq!(
        crate::servers::plain_path(std::path::PathBuf::from(r"\\?\C:\app\res")),
        std::path::PathBuf::from(r"C:\app\res")
    );
    assert_eq!(
        crate::servers::plain_path(std::path::PathBuf::from("/usr/lib/app")),
        std::path::PathBuf::from("/usr/lib/app")
    );
}
