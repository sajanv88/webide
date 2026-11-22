use std::fs;
use std::path::{Path, PathBuf};

use ide_core::fs::{
    LineEnding, NodeKind, ReadLimits, ReadOnlyReason, ReadOutcome, TEMP_FILE_PREFIX,
};
use ide_core::{IdeError, RelPath, WorkspaceFs, WorkspaceRoot};

use crate::LocalFs;

const LIMITS: ReadLimits = ReadLimits {
    read_only_above: 1024,
    refuse_above: 4096,
};
const SECRET: &str = "outside the workspace";

/// A temp folder with `ws/` (the workspace), `outside/secret.txt` and `ws-evil/`.
struct Fixture {
    _dir: tempfile::TempDir,
    base: PathBuf,
    root: WorkspaceRoot,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(dir.path()).unwrap();
        let ws = base.join("ws");
        fs::create_dir_all(ws.join("src").join("nested")).unwrap();
        fs::create_dir_all(ws.join("docs")).unwrap();
        fs::write(ws.join("src").join("main.ts"), "console.log(1);\n").unwrap();
        fs::write(ws.join("README.md"), "# hi\r\n").unwrap();
        fs::create_dir_all(base.join("outside")).unwrap();
        fs::write(base.join("outside").join("secret.txt"), SECRET).unwrap();
        fs::create_dir_all(base.join("ws-evil")).unwrap();
        fs::write(base.join("ws-evil").join("x.txt"), SECRET).unwrap();
        let root = WorkspaceRoot::from_canonical(fs::canonicalize(&ws).unwrap()).unwrap();
        Self {
            _dir: dir,
            base,
            root,
        }
    }

    fn ws(&self, rel: &str) -> PathBuf {
        rel.split('/')
            .fold(self.root.path().to_path_buf(), |p, s| p.join(s))
    }

    fn secret(&self) -> PathBuf {
        self.base.join("outside").join("secret.txt")
    }

    fn assert_secret_untouched(&self) {
        assert_eq!(fs::read_to_string(self.secret()).unwrap(), SECRET);
        assert_eq!(fs::read_dir(self.base.join("outside")).unwrap().count(), 1);
    }
}

fn rel(path: &str) -> RelPath {
    RelPath::parse(path).unwrap()
}

/// Symlinks need Developer Mode or admin rights on Windows; skip when refused.
fn try_symlink(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    let result = std::os::unix::fs::symlink(target, link);
    #[cfg(windows)]
    let result = if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    };
    match result {
        Ok(()) => true,
        Err(error) => {
            eprintln!("skipping symlink case: can't create symlinks here ({error})");
            false
        }
    }
}

#[test]
fn traversal_never_reaches_outside() {
    let fx = Fixture::new();
    let fs_ = LocalFs::new();
    let secret = fx.secret().to_string_lossy().into_owned();
    let inputs = [
        "..",
        "../outside/secret.txt",
        "../outside",
        "src/../../outside/secret.txt",
        "..\\outside\\secret.txt",
        "src\\..\\..\\outside\\secret.txt",
        "/etc/passwd",
        "C:\\Windows\\win.ini",
        "C:/Windows/win.ini",
        "\\\\?\\C:\\Windows\\win.ini",
        "\\\\localhost\\c$\\Windows\\win.ini",
        "./../outside/secret.txt",
        "src//../../outside",
        "../ws-evil/x.txt",
        secret.as_str(),
    ];
    for input in inputs {
        let Ok(path) = RelPath::parse(input) else {
            continue;
        }; // rejected without IO
        // Parsed (only possible on Unix for names with `\` or `:`): must not escape.
        assert!(
            !matches!(fs_.read_file(&fx.root, &path, LIMITS), Ok(ReadOutcome::Text { ref text, .. }) if text == SECRET),
            "{input:?} read the secret"
        );
        let _ = fs_.write_file(&fx.root, &path, "overwritten");
        let _ = fs_.list_dir(&fx.root, &path);
    }
    fx.assert_secret_untouched();
}

#[test]
fn symlinks_out_of_the_workspace_are_rejected() {
    let fx = Fixture::new();
    let fs_ = LocalFs::new();
    if !try_symlink(&fx.base.join("outside"), &fx.ws("out-dir"))
        || !try_symlink(&fx.secret(), &fx.ws("out-file"))
    {
        return;
    }

    let cases: &[(&str, &str)] = &[
        ("read through a folder link", "out-dir/secret.txt"),
        ("read a file link", "out-file"),
        ("list a folder link", "out-dir"),
        ("write a file link", "out-file"),
        ("write through a folder link", "out-dir/secret.txt"),
        ("create through a folder link", "out-dir/new.txt"),
    ];
    for (label, path) in cases {
        let path = rel(path);
        let outcome = if label.starts_with("read") {
            fs_.read_file(&fx.root, &path, LIMITS).map(|_| ())
        } else if label.starts_with("list") {
            fs_.list_dir(&fx.root, &path).map(|_| ())
        } else {
            fs_.write_file(&fx.root, &path, "overwritten").map(|_| ())
        };
        assert!(
            matches!(outcome, Err(IdeError::OutsideWorkspace)),
            "{label}: {outcome:?}"
        );
    }
    fx.assert_secret_untouched();

    // The listing still shows the links, flagged, and says what they point at.
    let listing = fs_.list_dir(&fx.root, &RelPath::root()).unwrap();
    let out_dir = listing
        .entries
        .iter()
        .find(|e| e.name == "out-dir")
        .unwrap();
    assert!(out_dir.is_symlink);
    assert_eq!(out_dir.kind, NodeKind::Directory);
}

#[test]
fn symlinks_inside_the_workspace_work() {
    let fx = Fixture::new();
    let fs_ = LocalFs::new();
    if !try_symlink(&fx.ws("src"), &fx.ws("src-link")) {
        return;
    }
    let outcome = fs_
        .read_file(&fx.root, &rel("src-link/main.ts"), LIMITS)
        .unwrap();
    assert!(matches!(outcome, ReadOutcome::Text { ref text, .. } if text == "console.log(1);\n"));
    fs_.write_file(&fx.root, &rel("src-link/main.ts"), "changed")
        .unwrap();
    assert_eq!(fs::read_to_string(fx.ws("src/main.ts")).unwrap(), "changed");
}

#[test]
fn a_sibling_with_the_same_prefix_is_outside() {
    let fx = Fixture::new();
    let fs_ = LocalFs::new();
    if !try_symlink(&fx.base.join("ws-evil"), &fx.ws("evil")) {
        return;
    }
    let outcome = fs_.read_file(&fx.root, &rel("evil/x.txt"), LIMITS);
    assert!(
        matches!(outcome, Err(IdeError::OutsideWorkspace)),
        "{outcome:?}"
    );
}

#[test]
fn writing_through_a_dangling_link_fails() {
    let fx = Fixture::new();
    let fs_ = LocalFs::new();
    if !try_symlink(
        &fx.base.join("outside").join("gone.txt"),
        &fx.ws("dangling"),
    ) {
        return;
    }
    let outcome = fs_.write_file(&fx.root, &rel("dangling"), "x");
    assert!(matches!(outcome, Err(IdeError::NotFound)), "{outcome:?}");
    assert!(!fx.base.join("outside").join("gone.txt").exists());
}

#[test]
fn lists_one_level_sorted_and_hides_temp_files() {
    let fx = Fixture::new();
    fs::write(fx.ws(&format!("{TEMP_FILE_PREFIX}123-0")), "partial").unwrap();
    let listing = LocalFs::new().list_dir(&fx.root, &RelPath::root()).unwrap();
    let names: Vec<_> = listing
        .entries
        .iter()
        .map(|e| (e.name.as_str(), e.kind))
        .collect();
    assert_eq!(
        names,
        [
            ("docs", NodeKind::Directory),
            ("src", NodeKind::Directory),
            ("README.md", NodeKind::File)
        ]
    );
    assert_eq!(listing.entries[2].size, Some(6));
    assert_eq!(listing.entries[1].path.as_str(), "src");
    assert!(!listing.truncated);

    let nested = LocalFs::new().list_dir(&fx.root, &rel("src")).unwrap();
    let paths: Vec<_> = nested.entries.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, ["src/nested", "src/main.ts"]);
}

#[test]
fn reports_what_can_and_cant_be_listed_or_read() {
    let fx = Fixture::new();
    let fs_ = LocalFs::new();
    assert!(matches!(
        fs_.list_dir(&fx.root, &rel("README.md")),
        Err(IdeError::NotADirectory)
    ));
    assert!(matches!(
        fs_.list_dir(&fx.root, &rel("missing")),
        Err(IdeError::NotFound)
    ));
    assert!(matches!(
        fs_.read_file(&fx.root, &rel("src"), LIMITS),
        Err(IdeError::NotAFile)
    ));
    assert!(matches!(
        fs_.read_file(&fx.root, &rel("missing.ts"), LIMITS),
        Err(IdeError::NotFound)
    ));
}

#[test]
fn reads_text_and_classifies_the_rest() {
    let fx = Fixture::new();
    fs::write(fx.ws("big.txt"), "x".repeat(2000)).unwrap();
    fs::write(fx.ws("huge.txt"), "x".repeat(5000)).unwrap();
    fs::write(fx.ws("image.png"), b"\x89PNG\r\n\x1a\n\0\0\0").unwrap();
    fs::write(fx.ws("latin1.txt"), b"caf\xe9").unwrap();
    let fs_ = LocalFs::new();

    match fs_.read_file(&fx.root, &rel("README.md"), LIMITS).unwrap() {
        ReadOutcome::Text {
            text,
            stamp,
            line_ending,
            read_only,
        } => {
            assert_eq!(text, "# hi\r\n");
            assert_eq!(stamp.size, 6);
            assert!(stamp.modified_ms.is_some());
            assert_eq!(line_ending, LineEnding::Crlf);
            assert_eq!(read_only, None);
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        fs_.read_file(&fx.root, &rel("big.txt"), LIMITS).unwrap(),
        ReadOutcome::Text {
            read_only: Some(ReadOnlyReason::TooLargeToEdit),
            ..
        }
    ));
    assert!(matches!(
        fs_.read_file(&fx.root, &rel("huge.txt"), LIMITS).unwrap(),
        ReadOutcome::TooLarge { limit: 4096, .. }
    ));
    assert!(matches!(
        fs_.read_file(&fx.root, &rel("image.png"), LIMITS).unwrap(),
        ReadOutcome::Binary { .. }
    ));
    assert!(matches!(
        fs_.read_file(&fx.root, &rel("latin1.txt"), LIMITS).unwrap(),
        ReadOutcome::NotUtf8 { .. }
    ));
}

#[test]
fn read_only_files_open_read_only_and_refuse_writes() {
    let fx = Fixture::new();
    let path = fx.ws("locked.txt");
    fs::write(&path, "locked").unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions.clone()).unwrap();

    let fs_ = LocalFs::new();
    assert!(matches!(
        fs_.read_file(&fx.root, &rel("locked.txt"), LIMITS).unwrap(),
        ReadOutcome::Text {
            read_only: Some(ReadOnlyReason::ReadOnlyOnDisk),
            ..
        }
    ));
    assert!(matches!(
        fs_.write_file(&fx.root, &rel("locked.txt"), "x"),
        Err(IdeError::PermissionDenied)
    ));
    assert_eq!(fs::read_to_string(&path).unwrap(), "locked");

    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    fs::set_permissions(&path, permissions).unwrap();
}

#[test]
fn writes_atomically_and_creates_new_files() {
    let fx = Fixture::new();
    let fs_ = LocalFs::new();

    let stamp = fs_
        .write_file(&fx.root, &rel("src/main.ts"), "export {};\n")
        .unwrap();
    assert_eq!(
        fs::read_to_string(fx.ws("src/main.ts")).unwrap(),
        "export {};\n"
    );
    assert_eq!(stamp.size, 11);

    fs_.write_file(&fx.root, &rel("docs/new.md"), "new")
        .unwrap();
    assert_eq!(fs::read_to_string(fx.ws("docs/new.md")).unwrap(), "new");

    let leftovers: Vec<_> = fs::read_dir(fx.ws("src"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(TEMP_FILE_PREFIX)
        })
        .collect();
    assert!(leftovers.is_empty(), "temp files left: {leftovers:?}");
}

#[test]
fn refuses_writes_that_arent_to_a_file() {
    let fx = Fixture::new();
    let fs_ = LocalFs::new();
    let cases: &[(&str, &str)] = &[
        ("", "NotAFile"),
        ("src", "NotAFile"),
        ("missing/new.txt", "NotFound"),
        ("README.md/child", "NotADirectory"),
    ];
    for (path, expected) in cases {
        let path = RelPath::parse(path).unwrap();
        let outcome = fs_.write_file(&fx.root, &path, "x");
        assert!(
            format!("{outcome:?}").contains(expected),
            "{path}: {outcome:?}"
        );
    }
}

#[test]
fn a_removed_root_is_not_found() {
    let fx = Fixture::new();
    fs::remove_dir_all(fx.root.path()).unwrap();
    let outcome = LocalFs::new().list_dir(&fx.root, &RelPath::root());
    assert!(matches!(outcome, Err(IdeError::NotFound)), "{outcome:?}");
}
