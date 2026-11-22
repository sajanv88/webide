//! On a real repository in a temporary folder, with the installed git.

use std::fs;
use std::path::Path;
use std::time::Duration;

use ide_core::git::{DiffSide, DiffTexts, GitChange, GitState, GitStatus};
use ide_core::{GitClient, IdeError, RelPath, WorkspaceRoot};
use tempfile::TempDir;

use crate::LocalGit;
use crate::run::{RunError, git};

fn p(path: &str) -> RelPath {
    RelPath::parse(path).unwrap()
}

fn root_of(path: &Path) -> WorkspaceRoot {
    WorkspaceRoot::from_canonical(fs::canonicalize(path).unwrap()).unwrap()
}

/// Runs git in `dir`, for setting a repository up.
fn sh(dir: &Path, args: &[&str]) {
    git(dir, args, None, Duration::from_secs(30)).unwrap_or_else(|e| panic!("git {args:?}: {e:?}"));
}

/// A new repository with an identity of its own, so commits work anywhere.
fn repo() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    sh(dir.path(), &["init", "--quiet", "--initial-branch=main"]);
    sh(dir.path(), &["config", "user.name", "Test"]);
    sh(
        dir.path(),
        &["config", "user.email", "test@example.invalid"],
    );
    sh(dir.path(), &["config", "commit.gpgsign", "false"]);
    sh(dir.path(), &["config", "core.autocrlf", "false"]);
    dir
}

fn status(root: &WorkspaceRoot) -> GitStatus {
    match LocalGit.state(root).unwrap() {
        GitState::Repository(status) => status,
        other => panic!("not a repository: {other:?}"),
    }
}

/// `path staged unstaged` for each changed file.
fn changes(root: &WorkspaceRoot) -> Vec<String> {
    status(root)
        .files
        .iter()
        .map(|file| format!("{} {:?} {:?}", file.path, file.staged, file.unstaged))
        .collect()
}

#[test]
fn a_folder_outside_any_repository_says_so() {
    let outside = tempfile::tempdir().unwrap();
    let state = LocalGit.state(&root_of(outside.path())).unwrap();
    // A temporary folder can sit inside someone's repository; then it's a repository.
    assert!(matches!(
        state,
        GitState::NotARepository | GitState::Repository(_)
    ));
}

#[test]
fn stage_commit_and_change_a_file() {
    let dir = repo();
    let root = root_of(dir.path());
    fs::write(dir.path().join("a.txt"), "one\n").unwrap();
    assert_eq!(changes(&root), ["a.txt None Some(Untracked)"]);

    LocalGit.stage(&root, &[p("a.txt")]).unwrap();
    assert_eq!(changes(&root), ["a.txt Some(Added) None"]);
    let head = status(&root).head;
    assert_eq!(head.branch.as_deref(), Some("main"));
    assert_eq!(head.commit, None);

    LocalGit.commit(&root, "First commit\n").unwrap();
    assert!(changes(&root).is_empty());
    assert_eq!(status(&root).head.commit.map(|c| c.len()), Some(7));

    fs::write(dir.path().join("a.txt"), "two\n").unwrap();
    assert_eq!(changes(&root), ["a.txt None Some(Modified)"]);
    assert_eq!(
        LocalGit
            .diff_texts(&root, &p("a.txt"), DiffSide::Unstaged, 1 << 20)
            .unwrap(),
        DiffTexts::Text {
            original: "one\n".into(),
            modified: "two\n".into()
        }
    );

    LocalGit.stage(&root, &[p("a.txt")]).unwrap();
    assert_eq!(
        LocalGit
            .diff_texts(&root, &p("a.txt"), DiffSide::Staged, 1 << 20)
            .unwrap(),
        DiffTexts::Text {
            original: "one\n".into(),
            modified: "two\n".into()
        }
    );
    LocalGit.unstage(&root, &[p("a.txt")]).unwrap();
    assert_eq!(changes(&root), ["a.txt None Some(Modified)"]);

    LocalGit.discard(&root, &[p("a.txt")]).unwrap();
    assert!(changes(&root).is_empty());
    assert_eq!(
        fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "one\n"
    );
}

#[test]
fn unstaging_before_the_first_commit_keeps_the_file() {
    let dir = repo();
    let root = root_of(dir.path());
    fs::write(dir.path().join("a.txt"), "one\n").unwrap();
    LocalGit.stage(&root, &[p("a.txt")]).unwrap();
    LocalGit.unstage(&root, &[p("a.txt")]).unwrap();
    assert_eq!(changes(&root), ["a.txt None Some(Untracked)"]);
    assert!(dir.path().join("a.txt").exists());
}

#[test]
fn a_deleted_file_is_staged_as_deleted() {
    let dir = repo();
    let root = root_of(dir.path());
    fs::write(dir.path().join("a.txt"), "one\n").unwrap();
    LocalGit.stage(&root, &[p("a.txt")]).unwrap();
    LocalGit.commit(&root, "add").unwrap();
    fs::remove_file(dir.path().join("a.txt")).unwrap();
    assert_eq!(changes(&root), ["a.txt None Some(Deleted)"]);
    assert_eq!(
        LocalGit
            .diff_texts(&root, &p("a.txt"), DiffSide::Unstaged, 1 << 20)
            .unwrap(),
        DiffTexts::Text {
            original: "one\n".into(),
            modified: String::new()
        }
    );
    LocalGit.stage(&root, &[p("a.txt")]).unwrap();
    assert_eq!(changes(&root), ["a.txt Some(Deleted) None"]);
}

#[test]
fn a_path_is_never_a_glob() {
    let dir = repo();
    let root = root_of(dir.path());
    fs::write(dir.path().join("a.txt"), "a").unwrap();
    fs::write(dir.path().join("b.txt"), "b").unwrap();
    // As a glob, `[ab].txt` would match both; as a path, there's no such file,
    // so nothing may be staged.
    assert!(LocalGit.stage(&root, &[p("[ab].txt")]).is_err());
    assert_eq!(
        changes(&root),
        ["a.txt None Some(Untracked)", "b.txt None Some(Untracked)"]
    );
}

#[test]
fn a_workspace_in_a_subfolder_sees_only_its_own_files() {
    let dir = repo();
    fs::create_dir_all(dir.path().join("web/src")).unwrap();
    fs::write(dir.path().join("web/src/a.ts"), "a").unwrap();
    fs::write(dir.path().join("other.txt"), "x").unwrap();
    let root = root_of(&dir.path().join("web"));
    assert_eq!(changes(&root), ["src/a.ts None Some(Untracked)"]);
    LocalGit.stage(&root, &[p("src/a.ts")]).unwrap();
    assert_eq!(changes(&root), ["src/a.ts Some(Added) None"]);
    assert_eq!(
        LocalGit
            .diff_texts(&root, &p("src/a.ts"), DiffSide::Staged, 1 << 20)
            .unwrap(),
        DiffTexts::Text {
            original: String::new(),
            modified: "a".into()
        }
    );
}

#[test]
fn binary_and_large_files_are_not_shown_as_text() {
    let dir = repo();
    let root = root_of(dir.path());
    fs::write(dir.path().join("bin.dat"), b"a\0b").unwrap();
    fs::write(dir.path().join("big.txt"), "x".repeat(100)).unwrap();
    assert_eq!(
        LocalGit
            .diff_texts(&root, &p("bin.dat"), DiffSide::Unstaged, 1 << 20)
            .unwrap(),
        DiffTexts::Binary
    );
    assert_eq!(
        LocalGit
            .diff_texts(&root, &p("big.txt"), DiffSide::Unstaged, 10)
            .unwrap(),
        DiffTexts::TooLarge { limit: 10 }
    );
    // A stored version is measured before it's read, too.
    LocalGit.stage(&root, &[p("big.txt")]).unwrap();
    assert_eq!(
        LocalGit
            .diff_texts(&root, &p("big.txt"), DiffSide::Staged, 10)
            .unwrap(),
        DiffTexts::TooLarge { limit: 10 }
    );
}

#[test]
fn branches_can_be_listed_created_and_switched() {
    let dir = repo();
    let root = root_of(dir.path());
    fs::write(dir.path().join("a.txt"), "a").unwrap();
    LocalGit.stage(&root, &[p("a.txt")]).unwrap();
    LocalGit.commit(&root, "first").unwrap();

    LocalGit.switch_branch(&root, "feature/x", true).unwrap();
    assert_eq!(status(&root).head.branch.as_deref(), Some("feature/x"));
    let branches: Vec<String> = LocalGit
        .branches(&root)
        .unwrap()
        .iter()
        .map(|b| format!("{}{}", if b.current { "*" } else { "" }, b.name))
        .collect();
    assert_eq!(branches, ["*feature/x", "main"]);

    LocalGit.switch_branch(&root, "main", false).unwrap();
    assert_eq!(status(&root).head.branch.as_deref(), Some("main"));
    assert!(matches!(
        LocalGit.switch_branch(&root, "--orphan", false),
        Err(IdeError::Git(_))
    ));
    assert!(matches!(
        LocalGit.switch_branch(&root, "nope", false),
        Err(IdeError::Git(_))
    ));
}

#[test]
fn a_commit_needs_a_message_and_something_staged() {
    let dir = repo();
    let root = root_of(dir.path());
    assert!(matches!(
        LocalGit.commit(&root, "  \n"),
        Err(IdeError::Git(_))
    ));
    // Nothing staged: git refuses, and says so.
    let Err(IdeError::Git(message)) = LocalGit.commit(&root, "nothing") else {
        panic!("expected git to refuse");
    };
    assert!(!message.is_empty());
}

#[test]
fn init_makes_a_repository() {
    let dir = tempfile::tempdir().unwrap();
    let root = root_of(dir.path());
    LocalGit.init(&root).unwrap();
    assert!(dir.path().join(".git").is_dir());
    assert!(matches!(
        LocalGit.state(&root).unwrap(),
        GitState::Repository(_)
    ));
}

#[test]
fn a_run_that_takes_too_long_is_stopped() {
    let dir = tempfile::tempdir().unwrap();
    // An alias that sleeps longer than the limit.
    let error = git(
        dir.path(),
        &["-c", "alias.wait=!sleep 5", "wait"],
        None,
        Duration::from_millis(200),
    );
    // Some systems have no `sleep` for the alias; then it fails at once instead.
    assert!(matches!(
        error,
        Err(RunError::TimedOut(_) | RunError::Failed { .. })
    ));
}

#[test]
fn changes_report_the_kind_of_each_side() {
    let dir = repo();
    let root = root_of(dir.path());
    fs::write(dir.path().join("a.txt"), "1").unwrap();
    LocalGit.stage(&root, &[p("a.txt")]).unwrap();
    LocalGit.commit(&root, "first").unwrap();
    fs::write(dir.path().join("a.txt"), "2").unwrap();
    LocalGit.stage(&root, &[p("a.txt")]).unwrap();
    fs::write(dir.path().join("a.txt"), "3").unwrap();
    let file = &status(&root).files[0];
    assert_eq!(file.staged, Some(GitChange::Modified));
    assert_eq!(file.unstaged, Some(GitChange::Modified));
}
