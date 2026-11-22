//! What the file system port returns: directory listings and file contents.

use crate::path::RelPath;

/// A listing stops here; the rest is reported as `truncated`.
pub const MAX_DIR_ENTRIES: usize = 10_000;

/// Names of the temporary files an atomic write creates beside its target.
/// Listings hide them and the watcher ignores them.
pub const TEMP_FILE_PREFIX: &str = ".~ide-tmp-";

/// How many leading bytes are checked for NUL when deciding a file is binary.
pub const BINARY_SNIFF_LEN: usize = 8000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    File,
    Directory,
    /// A device, a socket, a broken symlink, or anything else that isn't a file or a folder.
    Other,
}

/// One entry of a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileNode {
    pub name: String,
    pub path: RelPath,
    /// What it is, following a symlink.
    pub kind: NodeKind,
    pub is_symlink: bool,
    /// Size in bytes, for files.
    pub size: Option<u64>,
}

/// One level of a directory, sorted folders first, then by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirListing {
    pub path: RelPath,
    pub entries: Vec<FileNode>,
    /// More than [`MAX_DIR_ENTRIES`] entries: the rest were left out.
    pub truncated: bool,
    /// Entries left out because their names aren't valid UTF-8 or valid on this system.
    pub skipped: u32,
}

/// Size and modification time, to notice when a file changed underneath.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStamp {
    pub size: u64,
    /// Milliseconds since the Unix epoch, when the platform reports it.
    pub modified_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    Crlf,
    Mixed,
    /// No line break at all.
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadOnlyReason {
    /// Larger than the "read only above" setting.
    TooLargeToEdit,
    /// The file system says it can't be written.
    ReadOnlyOnDisk,
    /// It's inside a `.git` folder: git's own files, which the editor never writes.
    GitInternal,
}

/// Whether `path` is in, or is, a `.git` folder (any letter case, as Windows
/// and macOS file systems ignore it). Those files are git's: a hook or a
/// config line written there runs code the next time git does.
pub fn is_git_internal(path: &crate::RelPath) -> bool {
    path.segments()
        .any(|segment| segment.eq_ignore_ascii_case(".git"))
}

/// Size limits for opening a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadLimits {
    pub read_only_above: u64,
    pub refuse_above: u64,
}

/// The result of reading a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadOutcome {
    Text {
        text: String,
        stamp: FileStamp,
        line_ending: LineEnding,
        read_only: Option<ReadOnlyReason>,
    },
    /// Larger than `limit`: not read at all.
    TooLarge { stamp: FileStamp, limit: u64 },
    /// Has NUL bytes near the start.
    Binary { stamp: FileStamp },
    /// Text in an encoding other than UTF-8.
    NotUtf8 { stamp: FileStamp },
}

/// Sorts folders first, then by name without regard to case, then by exact name.
pub fn sort_nodes(nodes: &mut [FileNode]) {
    nodes.sort_by(|a, b| {
        let a_dir = a.kind == NodeKind::Directory;
        let b_dir = b.kind == NodeKind::Directory;
        b_dir
            .cmp(&a_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
}

pub fn detect_line_ending(text: &str) -> LineEnding {
    let crlf = text.matches("\r\n").count();
    let lf = text.matches('\n').count() - crlf;
    match (lf, crlf) {
        (0, 0) => LineEnding::None,
        (_, 0) => LineEnding::Lf,
        (0, _) => LineEnding::Crlf,
        _ => LineEnding::Mixed,
    }
}

/// A NUL byte in the first [`BINARY_SNIFF_LEN`] bytes means binary.
pub fn looks_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(BINARY_SNIFF_LEN).any(|byte| *byte == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str, kind: NodeKind) -> FileNode {
        FileNode {
            name: name.to_owned(),
            path: RelPath::parse_with(name, crate::NameRules::Unix).unwrap(),
            kind,
            is_symlink: false,
            size: None,
        }
    }

    #[test]
    fn sorts_folders_first_then_by_name() {
        let mut nodes = vec![
            node("b.ts", NodeKind::File),
            node("Src", NodeKind::Directory),
            node("A.md", NodeKind::File),
            node("a.md", NodeKind::File),
            node("docs", NodeKind::Directory),
        ];
        sort_nodes(&mut nodes);
        let names: Vec<_> = nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["docs", "Src", "A.md", "a.md", "b.ts"]);
    }

    #[test]
    fn detects_line_endings() {
        let cases = [
            ("", LineEnding::None),
            ("one line", LineEnding::None),
            ("a\nb\n", LineEnding::Lf),
            ("a\r\nb\r\n", LineEnding::Crlf),
            ("a\r\nb\n", LineEnding::Mixed),
        ];
        for (text, expected) in cases {
            assert_eq!(detect_line_ending(text), expected, "{text:?}");
        }
    }

    #[test]
    fn sniffs_binary() {
        assert!(!looks_binary(b"plain text\n"));
        assert!(looks_binary(b"PK\x03\x04\0\0"));
        let mut late_nul = vec![b'a'; BINARY_SNIFF_LEN];
        late_nul.push(0);
        assert!(!looks_binary(&late_nul));
    }
}

#[cfg(test)]
mod git_internal_tests {
    use super::is_git_internal;
    use crate::RelPath;

    #[test]
    fn any_git_segment_in_any_case() {
        for inside in [
            ".git",
            ".git/config",
            ".GIT/hooks/pre-commit",
            "sub/.git/HEAD",
        ] {
            assert!(
                is_git_internal(&RelPath::parse(inside).unwrap()),
                "{inside}"
            );
        }
        for outside in ["", "src/a.ts", ".gitignore", ".github/ci.yaml", "my.git/x"] {
            assert!(
                !is_git_internal(&RelPath::parse(outside).unwrap()),
                "{outside}"
            );
        }
    }
}
