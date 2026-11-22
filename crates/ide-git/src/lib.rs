//! Git for the open workspace, by running the person's own `git`.
//!
//! Every run goes straight to the `git` program (no shell), in the workspace
//! folder, with `--literal-pathspecs` (a path is never a glob), no terminal
//! prompts, and a time limit. Paths in and out are workspace-relative; files
//! of the repository outside the workspace are left out.

mod run;
mod status;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::time::Duration;

use ide_core::git::{DiffSide, DiffTexts, GitBranch, GitState, valid_branch_name};
use ide_core::workspace::display_path;
use ide_core::{GitClient, IdeError, RelPath, WorkspaceRoot};

use crate::run::{RunError, git};

/// How long reading git's state may take.
const READ_LIMIT: Duration = Duration::from_secs(30);
/// How long a change (stage, switch…) may take.
const WRITE_LIMIT: Duration = Duration::from_secs(60);
/// A commit runs the repository's hooks, which can take a while.
const COMMIT_LIMIT: Duration = Duration::from_secs(300);

/// [`GitClient`] on the installed `git`.
#[derive(Debug, Clone, Copy, Default)]
pub struct LocalGit;

impl LocalGit {
    pub fn new() -> Self {
        Self
    }
}

/// Where to run git: the workspace folder, as people write it (git doesn't
/// take Windows' `\\?\` paths).
fn folder(root: &WorkspaceRoot) -> PathBuf {
    PathBuf::from(display_path(root.path()))
}

fn failed(error: &RunError) -> IdeError {
    IdeError::Git(error.message())
}

fn args_with_paths<'a>(head: &[&'a str], paths: &'a [RelPath]) -> Vec<&'a str> {
    let mut args = head.to_vec();
    args.push("--");
    args.extend(paths.iter().map(RelPath::as_str));
    args
}

/// Whether the repository has a commit yet.
fn has_head(cwd: &Path) -> bool {
    git(
        cwd,
        &["rev-parse", "--verify", "--quiet", "HEAD"],
        None,
        READ_LIMIT,
    )
    .is_ok()
}

/// A version of a file from git: `rev` is `HEAD` or `""` (the index). `None`
/// when the file isn't in that version.
fn blob(cwd: &Path, rev: &str, path: &RelPath) -> Option<Vec<u8>> {
    // `./` makes the path relative to the workspace, not the repository's top.
    let spec = format!("{rev}:./{}", path.as_str());
    git(cwd, &["cat-file", "blob", &spec], None, READ_LIMIT)
        .ok()
        .map(|output| output.stdout)
}

/// The working-tree file, if it's there and inside the workspace.
fn working_file(root: &WorkspaceRoot, path: &RelPath) -> Result<Option<Vec<u8>>, IdeError> {
    let canonical_root = std::fs::canonicalize(root.path()).map_err(|e| IdeError::from_io(&e))?;
    let target = path.to_native(&canonical_root);
    let canonical = match std::fs::canonicalize(&target) {
        Ok(canonical) => canonical,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(IdeError::from_io(&error)),
    };
    if !canonical.starts_with(&canonical_root) {
        return Err(IdeError::OutsideWorkspace);
    }
    if canonical.is_dir() {
        return Ok(None);
    }
    std::fs::read(&canonical)
        .map(Some)
        .map_err(|error| IdeError::from_io(&error))
}

/// Two versions as texts, unless one is binary or too large.
fn texts(original: Option<Vec<u8>>, modified: Option<Vec<u8>>, limit: u64) -> DiffTexts {
    let sides = [original.unwrap_or_default(), modified.unwrap_or_default()];
    if sides
        .iter()
        .any(|side| u64::try_from(side.len()).unwrap_or(u64::MAX) > limit)
    {
        return DiffTexts::TooLarge { limit };
    }
    if sides.iter().any(|side| side.contains(&0)) {
        return DiffTexts::Binary;
    }
    let [original, modified] = sides;
    match (String::from_utf8(original), String::from_utf8(modified)) {
        (Ok(original), Ok(modified)) => DiffTexts::Text { original, modified },
        _ => DiffTexts::Binary,
    }
}

impl GitClient for LocalGit {
    fn state(&self, root: &WorkspaceRoot) -> Result<GitState, IdeError> {
        let cwd = folder(root);
        let prefix = match git(&cwd, &["rev-parse", "--show-prefix"], None, READ_LIMIT) {
            Ok(output) => String::from_utf8_lossy(&output.stdout).trim().to_owned(),
            Err(RunError::NotFound) => return Ok(GitState::Unavailable),
            // 128 is git's "fatal": here, not a repository.
            Err(RunError::Failed {
                code: Some(128),
                stderr,
            }) if stderr.contains("not a git repository") => return Ok(GitState::NotARepository),
            Err(error) => return Err(failed(&error)),
        };
        let output = git(
            &cwd,
            &[
                "status",
                "--porcelain=v2",
                "--branch",
                "-z",
                "--untracked-files=all",
                "--",
                ".",
            ],
            None,
            READ_LIMIT,
        )
        .map_err(|error| failed(&error))?;
        Ok(GitState::Repository(status::parse(&output.stdout, &prefix)))
    }

    fn diff_texts(
        &self,
        root: &WorkspaceRoot,
        path: &RelPath,
        side: DiffSide,
        limit: u64,
    ) -> Result<DiffTexts, IdeError> {
        let cwd = folder(root);
        Ok(match side {
            DiffSide::Staged => texts(blob(&cwd, "HEAD", path), blob(&cwd, "", path), limit),
            DiffSide::Unstaged => texts(blob(&cwd, "", path), working_file(root, path)?, limit),
        })
    }

    fn stage(&self, root: &WorkspaceRoot, paths: &[RelPath]) -> Result<(), IdeError> {
        if paths.is_empty() {
            return Ok(());
        }
        // `-A`: a deleted file is staged as deleted.
        git(
            &folder(root),
            &args_with_paths(&["add", "-A"], paths),
            None,
            WRITE_LIMIT,
        )
        .map(drop)
        .map_err(|error| failed(&error))
    }

    fn unstage(&self, root: &WorkspaceRoot, paths: &[RelPath]) -> Result<(), IdeError> {
        if paths.is_empty() {
            return Ok(());
        }
        let cwd = folder(root);
        // Before the first commit there's nothing to restore from: take the
        // files out of the index instead.
        let head: &[&str] = if has_head(&cwd) {
            &["restore", "--staged"]
        } else {
            &["rm", "--cached", "-r", "-q"]
        };
        git(&cwd, &args_with_paths(head, paths), None, WRITE_LIMIT)
            .map(drop)
            .map_err(|error| failed(&error))
    }

    fn discard(&self, root: &WorkspaceRoot, paths: &[RelPath]) -> Result<(), IdeError> {
        if paths.is_empty() {
            return Ok(());
        }
        // The working tree back to the index; untracked files aren't touched.
        git(
            &folder(root),
            &args_with_paths(&["restore"], paths),
            None,
            WRITE_LIMIT,
        )
        .map(drop)
        .map_err(|error| failed(&error))
    }

    fn commit(&self, root: &WorkspaceRoot, message: &str) -> Result<(), IdeError> {
        if message.trim().is_empty() {
            return Err(IdeError::Git("a commit needs a message".to_owned()));
        }
        // The message on stdin, as written: no quoting to get wrong.
        git(
            &folder(root),
            &["commit", "--file=-", "--cleanup=strip"],
            Some(message.as_bytes()),
            COMMIT_LIMIT,
        )
        .map(drop)
        .map_err(|error| failed(&error))
    }

    fn branches(&self, root: &WorkspaceRoot) -> Result<Vec<GitBranch>, IdeError> {
        let output = git(
            &folder(root),
            &[
                "for-each-ref",
                "--format=%(HEAD)%00%(refname:short)",
                "refs/heads",
            ],
            None,
            READ_LIMIT,
        )
        .map_err(|error| failed(&error))?;
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| {
                let (mark, name) = line.split_once('\0')?;
                Some(GitBranch {
                    name: name.to_owned(),
                    current: mark == "*",
                })
            })
            .collect())
    }

    fn switch_branch(
        &self,
        root: &WorkspaceRoot,
        name: &str,
        create: bool,
    ) -> Result<(), IdeError> {
        if !valid_branch_name(name) {
            return Err(IdeError::Git(format!("`{name}` isn't a valid branch name")));
        }
        let args: &[&str] = if create {
            &["switch", "--create", name]
        } else {
            &["switch", name]
        };
        git(&folder(root), args, None, WRITE_LIMIT)
            .map(drop)
            .map_err(|error| failed(&error))
    }

    fn init(&self, root: &WorkspaceRoot) -> Result<(), IdeError> {
        git(&folder(root), &["init"], None, WRITE_LIMIT)
            .map(drop)
            .map_err(|error| failed(&error))
    }
}
