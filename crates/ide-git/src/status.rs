//! Reading `git status --porcelain=v2 --branch -z`.
//! Format: https://git-scm.com/docs/git-status#_porcelain_format_version_2

use ide_core::RelPath;
use ide_core::git::{GitChange, GitFileStatus, GitHead, GitStatus, MAX_STATUS_FILES};

/// Parses the status output. Paths in it are relative to the repository's
/// top; `prefix` is where the workspace is under it (`git rev-parse
/// --show-prefix`: `""` or `"sub/dir/"`). Files outside the workspace, and
/// names a `RelPath` can't hold, are left out.
pub fn parse(output: &[u8], prefix: &str) -> GitStatus {
    let mut status = GitStatus::default();
    let text = String::from_utf8_lossy(output);
    let mut records = text.split('\0');
    while let Some(record) = records.next() {
        if let Some(header) = record.strip_prefix("# ") {
            read_header(&mut status.head, header);
            continue;
        }
        let file = match record.as_bytes().first() {
            // 1 XY sub mH mI mW hH hI path
            Some(b'1') => changed(record, 8, prefix, None),
            // 2 XY sub mH mI mW hH hI Xscore path, then the original path
            Some(b'2') => {
                let from = records.next().unwrap_or_default();
                changed(record, 9, prefix, Some(from))
            }
            // u XY sub m1 m2 m3 mW h1 h2 h3 path
            Some(b'u') => field(record, 10)
                .and_then(|path| relative(path, prefix))
                .map(|path| GitFileStatus {
                    path,
                    from: None,
                    staged: None,
                    unstaged: Some(GitChange::Conflicted),
                }),
            // ? path
            Some(b'?') => record
                .get(2..)
                .and_then(|path| relative(path, prefix))
                .map(|path| GitFileStatus {
                    path,
                    from: None,
                    staged: None,
                    unstaged: Some(GitChange::Untracked),
                }),
            _ => None,
        };
        if let Some(file) = file {
            if status.files.len() == MAX_STATUS_FILES {
                status.truncated = true;
                break;
            }
            status.files.push(file);
        }
    }
    status.files.sort_by(|a, b| a.path.cmp(&b.path));
    status
}

fn read_header(head: &mut GitHead, header: &str) {
    let (key, value) = header.split_once(' ').unwrap_or((header, ""));
    match key {
        "branch.oid" if value != "(initial)" => {
            head.commit = Some(value.chars().take(7).collect());
        }
        "branch.head" if value != "(detached)" => head.branch = Some(value.to_owned()),
        "branch.upstream" => head.upstream = Some(value.to_owned()),
        "branch.ab" => {
            for part in value.split(' ') {
                if let Some(n) = part.strip_prefix('+') {
                    head.ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = part.strip_prefix('-') {
                    head.behind = n.parse().unwrap_or(0);
                }
            }
        }
        _ => {}
    }
}

/// A `1` or `2` record: its path is everything after field `path_at`.
fn changed(
    record: &str,
    path_at: usize,
    prefix: &str,
    from: Option<&str>,
) -> Option<GitFileStatus> {
    let xy = record.get(2..4)?.as_bytes();
    let path = relative(field(record, path_at)?, prefix)?;
    Some(GitFileStatus {
        path,
        from: from.and_then(|from| relative(from, prefix)),
        staged: change(xy[0]),
        unstaged: change(xy[1]),
    })
}

/// Everything from the `index`-th space-separated field on (a path may hold spaces).
fn field(record: &str, index: usize) -> Option<&str> {
    record.splitn(index + 1, ' ').nth(index)
}

fn change(code: u8) -> Option<GitChange> {
    match code {
        b'M' => Some(GitChange::Modified),
        b'A' => Some(GitChange::Added),
        b'D' => Some(GitChange::Deleted),
        b'R' => Some(GitChange::Renamed),
        b'C' => Some(GitChange::Copied),
        b'T' => Some(GitChange::TypeChanged),
        b'U' => Some(GitChange::Conflicted),
        _ => None,
    }
}

/// A repository path as a workspace path, if it's in the workspace.
fn relative(path: &str, prefix: &str) -> Option<RelPath> {
    let inside = path.strip_prefix(prefix)?;
    // A folder git reports as untracked as a whole ends with `/`.
    RelPath::parse(inside.trim_end_matches('/')).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(path: &str) -> RelPath {
        RelPath::parse(path).unwrap()
    }

    #[test]
    fn reads_the_branch_and_every_kind_of_record() {
        let output = [
            "# branch.oid 0123456789abcdef0123456789abcdef01234567",
            "# branch.head main",
            "# branch.upstream origin/main",
            "# branch.ab +2 -1",
            "1 .M N... 100644 100644 100644 aaa bbb src/a b.ts",
            "1 A. N... 000000 100644 100644 000 ccc src/new.ts",
            "1 MD N... 100644 100644 000000 ddd eee gone.ts",
            "2 R. N... 100644 100644 100644 fff fff R100 src/renamed.ts",
            "src/old.ts",
            "u UU N... 100644 100644 100644 100644 111 222 333 both.ts",
            "? notes.md",
            "",
        ]
        .join("\0");
        let status = parse(output.as_bytes(), "");
        assert_eq!(
            status.head,
            GitHead {
                branch: Some("main".into()),
                commit: Some("0123456".into()),
                upstream: Some("origin/main".into()),
                ahead: 2,
                behind: 1,
            }
        );
        let files: Vec<String> = status
            .files
            .iter()
            .map(|file| {
                format!(
                    "{} {:?} {:?} {:?}",
                    file.path,
                    file.staged,
                    file.unstaged,
                    file.from.as_ref().map(RelPath::as_str)
                )
            })
            .collect();
        assert_eq!(
            files,
            [
                "both.ts None Some(Conflicted) None",
                "gone.ts Some(Modified) Some(Deleted) None",
                "notes.md None Some(Untracked) None",
                "src/a b.ts None Some(Modified) None",
                "src/new.ts Some(Added) None None",
                "src/renamed.ts Some(Renamed) None Some(\"src/old.ts\")",
            ]
        );
        assert!(!status.truncated);
    }

    #[test]
    fn a_new_repository_and_a_detached_head() {
        let status = parse(b"# branch.oid (initial)\0# branch.head main\0", "");
        assert_eq!(status.head.commit, None);
        assert_eq!(status.head.branch.as_deref(), Some("main"));
        let status = parse(b"# branch.oid abcdef0123\0# branch.head (detached)\0", "");
        assert_eq!(status.head.branch, None);
        assert_eq!(status.head.commit.as_deref(), Some("abcdef0"));
    }

    #[test]
    fn paths_are_made_relative_to_the_workspace_and_outsiders_dropped() {
        let output = "1 .M N... 100644 100644 100644 a b web/src/a.ts\0? web/notes.md\0? other/x.ts\0? web/build/\0";
        let status = parse(output.as_bytes(), "web/");
        let paths: Vec<&str> = status.files.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(paths, ["build", "notes.md", "src/a.ts"]);
        assert_eq!(status.files[0].path, p("build"));
    }
}
