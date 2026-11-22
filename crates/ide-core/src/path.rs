//! Workspace-relative paths, validated without touching the file system.
//!
//! Paths cross the IPC boundary as `/`-separated strings relative to the open
//! workspace root. [`RelPath::parse`] is the first of two guards: it rejects
//! anything that could name a place outside the root before any IO happens.
//! The second guard (canonicalise, then check containment) lives in `ide-fs`,
//! because following symlinks needs the file system.

use std::fmt;
use std::path::{Path, PathBuf};

/// The longest relative path accepted, in bytes.
pub const MAX_REL_PATH_LEN: usize = 4096;

/// A path inside a workspace, relative to its root.
///
/// Always `/`-separated, never absolute, with no empty, `.` or `..` segment.
/// The empty path is the root itself.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct RelPath(String);

/// Why a string isn't a valid [`RelPath`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PathError {
    #[error("the path is absolute")]
    Absolute,
    #[error("the path has an empty segment")]
    EmptySegment,
    #[error("the path has a `.` or `..` segment")]
    DotSegment,
    #[error("the path contains a control character")]
    ControlCharacter,
    #[error("`{0}` isn't allowed in a file name on this system")]
    ReservedCharacter(char),
    #[error("`{0}` is a reserved file name on Windows")]
    ReservedName(String),
    #[error("a file name can't end with a dot or a space on Windows")]
    TrailingDotOrSpace,
    #[error("the path is longer than {MAX_REL_PATH_LEN} bytes")]
    TooLong,
    #[error("`{0}` is not a single file name")]
    NotASingleName(String),
}

/// The naming rules of the file system the path will be used on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameRules {
    /// Windows: no `\ : * ? " < > |`, no control characters, no reserved device
    /// names, no trailing dot or space. (`:` also rules out NTFS alternate streams.)
    Windows,
    /// Unix: anything but `/` and NUL.
    Unix,
}

impl NameRules {
    /// The rules of the system this binary runs on.
    pub const fn native() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Unix
        }
    }
}

const WINDOWS_RESERVED_CHARS: &[char] = &['\\', ':', '*', '?', '"', '<', '>', '|'];
const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "CONIN$",
    "CONOUT$",
];

impl RelPath {
    /// The workspace root.
    pub fn root() -> Self {
        Self(String::new())
    }

    /// Parses a path with the rules of the system this binary runs on.
    pub fn parse(input: &str) -> Result<Self, PathError> {
        Self::parse_with(input, NameRules::native())
    }

    /// Parses a path with the given naming rules.
    pub fn parse_with(input: &str, rules: NameRules) -> Result<Self, PathError> {
        if input.is_empty() {
            return Ok(Self::root());
        }
        if input.len() > MAX_REL_PATH_LEN {
            return Err(PathError::TooLong);
        }
        if input.starts_with('/') {
            return Err(PathError::Absolute);
        }
        for segment in input.split('/') {
            check_segment(segment, rules)?;
        }
        Ok(Self(input.to_owned()))
    }

    /// The path as a `/`-separated string; empty for the root.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    /// The segments, from the root down. None for the root.
    pub fn segments(&self) -> impl Iterator<Item = &str> {
        self.0.split('/').filter(|segment| !segment.is_empty())
    }

    /// The last segment, or `None` for the root.
    pub fn file_name(&self) -> Option<&str> {
        self.segments().last()
    }

    /// The containing path, or `None` for the root.
    pub fn parent(&self) -> Option<Self> {
        if self.is_root() {
            return None;
        }
        Some(match self.0.rfind('/') {
            Some(index) => Self(self.0[..index].to_owned()),
            None => Self::root(),
        })
    }

    /// This path with one more segment. `name` must be a single valid name.
    pub fn join(&self, name: &str) -> Result<Self, PathError> {
        self.join_with(name, NameRules::native())
    }

    pub fn join_with(&self, name: &str, rules: NameRules) -> Result<Self, PathError> {
        if name.contains('/') || name.is_empty() {
            return Err(PathError::NotASingleName(name.to_owned()));
        }
        check_segment(name, rules)?;
        let joined = if self.is_root() {
            name.to_owned()
        } else {
            format!("{}/{name}", self.0)
        };
        if joined.len() > MAX_REL_PATH_LEN {
            return Err(PathError::TooLong);
        }
        Ok(Self(joined))
    }

    /// `base` with each segment appended, using the platform's separator.
    /// Pure path arithmetic: nothing is read from the disk.
    pub fn to_native(&self, base: &Path) -> PathBuf {
        let mut path = base.to_path_buf();
        for segment in self.segments() {
            path.push(segment);
        }
        path
    }
}

impl fmt::Display for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn check_segment(segment: &str, rules: NameRules) -> Result<(), PathError> {
    if segment.is_empty() {
        return Err(PathError::EmptySegment);
    }
    if segment == "." || segment == ".." {
        return Err(PathError::DotSegment);
    }
    match rules {
        NameRules::Unix => {
            if segment.contains('\0') {
                return Err(PathError::ControlCharacter);
            }
        }
        NameRules::Windows => {
            if segment.chars().any(|c| c.is_control()) {
                return Err(PathError::ControlCharacter);
            }
            if let Some(c) = segment.chars().find(|c| WINDOWS_RESERVED_CHARS.contains(c)) {
                return Err(PathError::ReservedCharacter(c));
            }
            if segment.ends_with('.') || segment.ends_with(' ') {
                return Err(PathError::TrailingDotOrSpace);
            }
            // `CON`, `con.txt` and `Con .log` all name the console device.
            let stem = segment.split('.').next().unwrap_or(segment).trim_end();
            if WINDOWS_RESERVED_NAMES
                .iter()
                .any(|name| name.eq_ignore_ascii_case(stem))
            {
                return Err(PathError::ReservedName(segment.to_owned()));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use NameRules::{Unix, Windows};

    #[test]
    fn accepts_ordinary_paths() {
        for rules in [Windows, Unix] {
            for input in [
                "",
                "a",
                "src/main.ts",
                "a/b/c.d.ts",
                ".gitignore",
                "node_modules/.bin",
                "名前/ファイル.txt",
                "with space/x y.md",
            ] {
                assert_eq!(
                    RelPath::parse_with(input, rules).map(|p| p.0),
                    Ok(input.to_owned()),
                    "{input:?} with {rules:?}"
                );
            }
        }
    }

    #[test]
    fn rejects_escapes_and_invalid_names() {
        let cases: &[(&str, NameRules, PathError)] = &[
            // Absolute paths
            ("/etc/passwd", Unix, PathError::Absolute),
            ("/etc/passwd", Windows, PathError::Absolute),
            ("C:/Windows", Windows, PathError::ReservedCharacter(':')),
            ("C:\\Windows", Windows, PathError::ReservedCharacter(':')),
            (
                "\\\\server\\share",
                Windows,
                PathError::ReservedCharacter('\\'),
            ),
            ("\\\\?\\C:\\x", Windows, PathError::ReservedCharacter('\\')),
            // Traversal
            ("..", Unix, PathError::DotSegment),
            ("../secret", Unix, PathError::DotSegment),
            ("a/../../b", Windows, PathError::DotSegment),
            ("a/./b", Unix, PathError::DotSegment),
            (
                "..\\..\\Windows\\win.ini",
                Windows,
                PathError::ReservedCharacter('\\'),
            ),
            ("a\\..\\b", Windows, PathError::ReservedCharacter('\\')),
            // Empty segments
            ("a//b", Unix, PathError::EmptySegment),
            ("a/", Windows, PathError::EmptySegment),
            // Control characters
            ("a\0b", Unix, PathError::ControlCharacter),
            ("a\0b", Windows, PathError::ControlCharacter),
            ("a\nb", Windows, PathError::ControlCharacter),
            // Windows-only names
            (
                "file.txt:secret",
                Windows,
                PathError::ReservedCharacter(':'),
            ),
            ("what?.md", Windows, PathError::ReservedCharacter('?')),
            ("CON", Windows, PathError::ReservedName("CON".to_owned())),
            (
                "src/nul.txt",
                Windows,
                PathError::ReservedName("nul.txt".to_owned()),
            ),
            (
                "Com1 .log",
                Windows,
                PathError::ReservedName("Com1 .log".to_owned()),
            ),
            ("trailing.", Windows, PathError::TrailingDotOrSpace),
            ("trailing ", Windows, PathError::TrailingDotOrSpace),
        ];
        for (input, rules, expected) in cases {
            assert_eq!(
                RelPath::parse_with(input, *rules),
                Err(expected.clone()),
                "{input:?} with {rules:?}"
            );
        }
    }

    #[test]
    fn unix_allows_what_windows_forbids() {
        for input in ["a\\b", "C:", "file.txt:x", "CON", "trailing."] {
            assert!(RelPath::parse_with(input, Unix).is_ok(), "{input:?}");
        }
    }

    #[test]
    fn rejects_overlong_paths() {
        let long = "a/".repeat(MAX_REL_PATH_LEN / 2) + "a";
        assert_eq!(RelPath::parse_with(&long, Unix), Err(PathError::TooLong));
    }

    #[test]
    fn navigates() {
        let path = RelPath::parse_with("src/lib/x.ts", Unix).unwrap();
        assert_eq!(path.file_name(), Some("x.ts"));
        assert_eq!(path.parent().unwrap().as_str(), "src/lib");
        assert_eq!(path.segments().collect::<Vec<_>>(), ["src", "lib", "x.ts"]);
        assert_eq!(
            RelPath::parse_with("top", Unix).unwrap().parent(),
            Some(RelPath::root())
        );
        assert_eq!(RelPath::root().parent(), None);
        assert_eq!(RelPath::root().file_name(), None);
        assert_eq!(RelPath::root().join_with("a", Unix).unwrap().as_str(), "a");
        assert_eq!(
            path.parent()
                .unwrap()
                .join_with("y.ts", Unix)
                .unwrap()
                .as_str(),
            "src/lib/y.ts"
        );
    }

    #[test]
    fn join_takes_one_valid_name() {
        let root = RelPath::root();
        for (name, expected) in [
            ("a/b", PathError::NotASingleName("a/b".to_owned())),
            ("", PathError::NotASingleName(String::new())),
            ("..", PathError::DotSegment),
        ] {
            assert_eq!(root.join_with(name, Unix), Err(expected), "{name:?}");
        }
        assert_eq!(
            root.join_with("x:y", Windows),
            Err(PathError::ReservedCharacter(':'))
        );
    }

    #[test]
    fn to_native_appends_segments() {
        let base = Path::new("base");
        let joined = RelPath::parse_with("a/b.txt", Unix)
            .unwrap()
            .to_native(base);
        assert_eq!(joined, base.join("a").join("b.txt"));
        assert_eq!(RelPath::root().to_native(base), base);
    }
}
