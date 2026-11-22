use std::fs;
use std::path::Path;

use ide_core::search::MAX_MATCHES;
use ide_core::{CancelFlag, IdeError, TextQuery, WorkspaceRoot, WorkspaceSearch};
use tempfile::TempDir;

use crate::LocalSearch;

fn workspace(files: &[(&str, &str)]) -> (TempDir, WorkspaceRoot) {
    let dir = tempfile::tempdir().expect("temp dir");
    for (path, text) in files {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().expect("parent")).expect("folders");
        fs::write(path, text).expect("write");
    }
    let root = WorkspaceRoot::from_canonical(fs::canonicalize(dir.path()).expect("canonical"))
        .expect("root");
    (dir, root)
}

fn query(pattern: &str) -> TextQuery {
    TextQuery {
        pattern: pattern.to_owned(),
        is_regex: false,
        match_case: false,
        whole_word: false,
        include: Vec::new(),
        exclude: Vec::new(),
        max_file_bytes: 1 << 20,
    }
}

/// `path:line:preview` for each matching line, in order.
fn found(root: &WorkspaceRoot, query: &TextQuery) -> Vec<String> {
    let outcome = LocalSearch
        .search_text(root, query, &CancelFlag::new())
        .expect("search");
    outcome
        .files
        .iter()
        .flat_map(|file| {
            file.lines
                .iter()
                .map(move |line| format!("{}:{}:{}", file.path.as_str(), line.line, line.preview))
        })
        .collect()
}

fn listed(root: &WorkspaceRoot) -> Vec<String> {
    LocalSearch
        .list_files(root, &CancelFlag::new())
        .expect("list")
        .files
        .iter()
        .map(|path| path.as_str().to_owned())
        .collect()
}

#[test]
fn finds_lines_in_path_order_with_line_numbers() {
    let (_dir, root) = workspace(&[
        ("b.ts", "nothing\nconst Needle = 1;\n"),
        ("a/c.ts", "needle()\n"),
        ("a.ts", "x\ny\nneedle; needle\n"),
    ]);
    assert_eq!(
        found(&root, &query("needle")),
        [
            "a/c.ts:1:needle()",
            "a.ts:3:needle; needle",
            "b.ts:2:const Needle = 1;",
        ]
    );
}

#[test]
fn reports_every_match_on_a_line_as_spans() {
    let (_dir, root) = workspace(&[("a.txt", "ab ab ab\n")]);
    let outcome = LocalSearch
        .search_text(&root, &query("ab"), &CancelFlag::new())
        .expect("search");
    let line = &outcome.files[0].lines[0];
    let spans: Vec<(u32, u32)> = line.spans.iter().map(|s| (s.start, s.end)).collect();
    assert_eq!(spans, [(0, 2), (3, 5), (6, 8)]);
    assert_eq!(line.column, 1);
}

#[test]
fn case_word_and_regex_options() {
    let (_dir, root) = workspace(&[("a.txt", "Item\nitem\nitems\nit3m\n")]);
    let cases: &[(TextQuery, &[&str])] = &[
        (
            query("item"),
            &["a.txt:1:Item", "a.txt:2:item", "a.txt:3:items"],
        ),
        (
            TextQuery {
                match_case: true,
                ..query("item")
            },
            &["a.txt:2:item", "a.txt:3:items"],
        ),
        (
            TextQuery {
                whole_word: true,
                ..query("item")
            },
            &["a.txt:1:Item", "a.txt:2:item"],
        ),
        (
            TextQuery {
                is_regex: true,
                ..query(r"it\dm")
            },
            &["a.txt:4:it3m"],
        ),
        // Not a regex: the dot and the backslash are just characters.
        (query(r"it\dm"), &[]),
    ];
    for (query, expected) in cases {
        assert_eq!(found(&root, query), *expected, "{query:?}");
    }
}

#[test]
fn an_invalid_regex_is_an_invalid_query() {
    let (_dir, root) = workspace(&[("a.txt", "x\n")]);
    let result = LocalSearch.search_text(
        &root,
        &TextQuery {
            is_regex: true,
            ..query("(")
        },
        &CancelFlag::new(),
    );
    assert!(matches!(result, Err(IdeError::InvalidQuery(_))));
}

#[test]
fn an_empty_pattern_finds_nothing() {
    let (_dir, root) = workspace(&[("a.txt", "x\n")]);
    assert!(found(&root, &query("")).is_empty());
}

#[test]
fn ignored_files_git_and_binary_files_are_skipped() {
    let (_dir, root) = workspace(&[
        (".gitignore", "dist/\n*.log\n"),
        ("dist/out.js", "needle\n"),
        ("debug.log", "needle\n"),
        (".git/config", "needle\n"),
        ("bin.dat", "needle\0\0\0"),
        (".github/ci.yaml", "needle\n"),
        ("src/a.ts", "needle\n"),
    ]);
    assert_eq!(
        found(&root, &query("needle")),
        [".github/ci.yaml:1:needle", "src/a.ts:1:needle"]
    );
    assert_eq!(
        listed(&root),
        [".github/ci.yaml", ".gitignore", "bin.dat", "src/a.ts"]
    );
}

#[test]
fn include_and_exclude_globs() {
    let (_dir, root) = workspace(&[
        ("src/a.ts", "needle\n"),
        ("src/a.test.ts", "needle\n"),
        ("docs/a.md", "needle\n"),
    ]);
    let only_ts = TextQuery {
        include: vec!["*.ts".into()],
        ..query("needle")
    };
    assert_eq!(
        found(&root, &only_ts),
        ["src/a.test.ts:1:needle", "src/a.ts:1:needle"]
    );
    let no_tests = TextQuery {
        include: vec!["*.ts".into()],
        exclude: vec!["*.test.ts".into(), " ".into()],
        ..query("needle")
    };
    assert_eq!(found(&root, &no_tests), ["src/a.ts:1:needle"]);
    let no_src = TextQuery {
        exclude: vec!["src/".into()],
        ..query("needle")
    };
    assert_eq!(found(&root, &no_src), ["docs/a.md:1:needle"]);
}

#[test]
fn an_include_glob_doesnt_bring_back_ignored_files() {
    let (_dir, root) = workspace(&[
        (".gitignore", "node_modules/\n"),
        ("node_modules/x/index.js", "needle\n"),
        ("index.js", "needle\n"),
    ]);
    let js = TextQuery {
        include: vec!["*.js".into()],
        ..query("needle")
    };
    assert_eq!(found(&root, &js), ["index.js:1:needle"]);
}

#[test]
fn files_over_the_size_limit_are_skipped() {
    let (_dir, root) = workspace(&[
        ("big.txt", "needle needle needle\n"),
        ("small.txt", "needle\n"),
    ]);
    let small_only = TextQuery {
        max_file_bytes: 10,
        ..query("needle")
    };
    assert_eq!(found(&root, &small_only), ["small.txt:1:needle"]);
}

#[test]
fn stops_at_the_match_limit_and_says_so() {
    let text = "needle\n".repeat(MAX_MATCHES + 5);
    let (_dir, root) = workspace(&[("a.txt", &text), ("b.txt", "needle\n")]);
    let outcome = LocalSearch
        .search_text(&root, &query("needle"), &CancelFlag::new())
        .expect("search");
    let lines: usize = outcome.files.iter().map(|file| file.lines.len()).sum();
    assert_eq!(lines, MAX_MATCHES);
    assert!(outcome.truncated);
    // Files are searched in parallel, so which were reached varies; what's
    // kept is the first lines in path order.
    assert_eq!(outcome.files[0].path.as_str(), "a.txt");
}

#[test]
fn a_cancelled_search_stops_and_says_so() {
    let (_dir, root) = workspace(&[("a.txt", "needle\n")]);
    let cancel = CancelFlag::new();
    cancel.cancel();
    let outcome = LocalSearch
        .search_text(&root, &query("needle"), &cancel)
        .expect("search");
    assert!(outcome.cancelled);
    assert!(outcome.files.is_empty());
}

#[test]
fn ignore_files_above_the_root_dont_apply() {
    let outer = tempfile::tempdir().expect("temp dir");
    fs::write(outer.path().join(".gitignore"), "*.ts\n").expect("write");
    let inner = outer.path().join("project");
    fs::create_dir(&inner).expect("folder");
    fs::write(inner.join("a.ts"), "needle\n").expect("write");
    let root =
        WorkspaceRoot::from_canonical(fs::canonicalize(&inner).expect("canonical")).expect("root");
    assert_eq!(found(&root, &query("needle")), ["a.ts:1:needle"]);
}

#[cfg(unix)]
#[test]
fn symlinks_are_not_followed() {
    let outside = tempfile::tempdir().expect("temp dir");
    fs::write(outside.path().join("secret.txt"), "needle\n").expect("write");
    let (dir, root) = workspace(&[("a.txt", "needle\n")]);
    std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).expect("symlink");
    std::os::unix::fs::symlink(
        outside.path().join("secret.txt"),
        dir.path().join("file-link.txt"),
    )
    .expect("symlink");
    assert_eq!(found(&root, &query("needle")), ["a.txt:1:needle"]);
    assert_eq!(listed(&root), ["a.txt"]);
}

#[test]
fn a_missing_root_is_not_found() {
    let (dir, root) = workspace(&[]);
    let gone: &Path = dir.path();
    fs::remove_dir_all(gone).expect("remove");
    let result = LocalSearch.list_files(&root, &CancelFlag::new());
    assert!(matches!(result, Err(IdeError::NotFound)));
}
