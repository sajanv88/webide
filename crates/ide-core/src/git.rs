//! Git, as the IDE sees it: the state of the open workspace's repository, and
//! the operations on it. The implementation (`ide-git`) runs the person's own
//! `git` program.

use crate::path::RelPath;

/// At most this many changed files are reported.
pub const MAX_STATUS_FILES: usize = 5_000;

/// What happened to a file, on one side (the index, or the working tree).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitChange {
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
    /// It changed kind: a file became a symlink, say.
    TypeChanged,
    /// Not tracked by git yet.
    Untracked,
    /// Both sides of a merge changed it.
    Conflicted,
}

/// One changed file. Paths are workspace-relative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitFileStatus {
    pub path: RelPath,
    /// Where it was renamed or copied from, when that's in the workspace too.
    pub from: Option<RelPath>,
    /// Its change in the index (what the next commit has): staged.
    pub staged: Option<GitChange>,
    /// Its change in the working tree that isn't staged.
    pub unstaged: Option<GitChange>,
}

/// The branch the working tree is on.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GitHead {
    /// The branch, or `None` when the head is detached.
    pub branch: Option<String>,
    /// The commit, abbreviated; `None` before the first commit.
    pub commit: Option<String>,
    /// The upstream branch, when there is one.
    pub upstream: Option<String>,
    /// Commits on this branch that the upstream doesn't have, and the reverse.
    pub ahead: u32,
    pub behind: u32,
}

/// The repository the workspace is in, and what changed in the workspace.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GitStatus {
    pub head: GitHead,
    /// Changed files inside the workspace, in path order.
    pub files: Vec<GitFileStatus>,
    /// It stopped at [`MAX_STATUS_FILES`].
    pub truncated: bool,
}

/// Whether the workspace can use git at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitState {
    /// The `git` program wasn't found.
    Unavailable,
    /// The workspace isn't inside a git repository.
    NotARepository,
    Repository(GitStatus),
}

/// Which two versions of a file to compare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffSide {
    /// The last commit and the index: what's staged.
    Staged,
    /// The index and the working tree: what isn't staged.
    Unstaged,
}

/// The two texts of a file to show side by side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffTexts {
    /// Empty where the file doesn't exist on that side (added, deleted).
    Text { original: String, modified: String },
    /// One side holds a NUL byte or isn't UTF-8.
    Binary,
    /// One side is larger than the limit.
    TooLarge { limit: u64 },
}

/// A local branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitBranch {
    pub name: String,
    pub current: bool,
}

/// Checks a branch name the way git does (`git check-ref-format --branch`),
/// for the rules people meet: no spaces or `~^:?*[\`, no `..` or `@{`, not
/// starting with `-` or `/` or ending with `/`, `.` or `.lock`, no empty part.
pub fn valid_branch_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.starts_with('/')
        && !name.ends_with('/')
        && !name.ends_with('.')
        && !name.ends_with(".lock")
        && !name.contains("..")
        && !name.contains("@{")
        && !name.contains("//")
        && name != "@"
        && name
            .split('/')
            .all(|part| !part.is_empty() && !part.starts_with('.'))
        && !name
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || "~^:?*[\\".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_names() {
        for good in [
            "main",
            "feature/search",
            "fix-12",
            "release/2026.11",
            "a/b/c",
        ] {
            assert!(valid_branch_name(good), "{good}");
        }
        for bad in [
            "",
            "-x",
            "/x",
            "x/",
            "x.",
            "x.lock",
            "a..b",
            "a@{b",
            "a//b",
            "@",
            "a b",
            "a~b",
            "a^b",
            "a:b",
            "a?b",
            "a*b",
            "a[b",
            "a\\b",
            ".hidden",
            "a/.b",
            "tab\tname",
        ] {
            assert!(!valid_branch_name(bad), "{bad:?}");
        }
    }
}
