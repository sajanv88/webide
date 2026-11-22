//! Searching a workspace: text in its files, and the list of its files.
//!
//! Both skip what `.gitignore` (and `.ignore`) leave out, and the `.git` folder.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::path::RelPath;

/// At most this many matching lines are returned by one search.
pub const MAX_MATCHES: usize = 10_000;
/// At most this many files are listed for quick open.
pub const MAX_LISTED_FILES: usize = 50_000;
/// A matching line longer than this (in UTF-16 units) is cut around its first match.
pub const MAX_PREVIEW_UNITS: usize = 240;
/// How much of the line is kept before the first match when a preview is cut.
pub const PREVIEW_LEAD_UNITS: usize = 40;

/// What to search for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextQuery {
    pub pattern: String,
    /// A regular expression (Rust syntax), rather than the text as written.
    pub is_regex: bool,
    pub match_case: bool,
    pub whole_word: bool,
    /// Globs (gitignore syntax); when any are given, only matching files are searched.
    pub include: Vec<String>,
    /// Globs (gitignore syntax) of files to leave out.
    pub exclude: Vec<String>,
    /// Files larger than this many bytes are skipped.
    pub max_file_bytes: u64,
}

/// A span inside a preview, in UTF-16 code units (as JavaScript counts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

/// One line that matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineMatch {
    /// 1-based.
    pub line: u32,
    /// 1-based, in UTF-16 units of the whole line: where the first match starts.
    pub column: u32,
    /// The line, or the part of it around the first match when it's long.
    pub preview: String,
    /// Every match in `preview`.
    pub spans: Vec<Span>,
}

/// The matching lines of one file, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMatches {
    pub path: RelPath,
    pub lines: Vec<LineMatch>,
}

/// What a text search found, files in path order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextSearchOutcome {
    pub files: Vec<FileMatches>,
    /// It stopped at [`MAX_MATCHES`] lines; there may be more.
    pub truncated: bool,
    /// A newer search replaced this one before it finished.
    pub cancelled: bool,
}

/// The workspace's files, in path order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileList {
    pub files: Vec<RelPath>,
    /// It stopped at [`MAX_LISTED_FILES`].
    pub truncated: bool,
}

/// Tells a running search to stop. Cheap to clone; every clone is the same flag.
#[derive(Debug, Clone, Default)]
pub struct CancelFlag(Arc<AtomicBool>);

impl CancelFlag {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Converts a byte offset in `text` to UTF-16 units.
fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// Builds a [`LineMatch`] from a line and the byte ranges that matched in it.
/// The line's ending is removed; long lines are cut around the first match.
pub fn line_match(line_number: u32, line: &str, byte_ranges: &[(usize, usize)]) -> LineMatch {
    let line = line.trim_end_matches(['\n', '\r']);
    let to_units = |byte: usize| utf16_len(&line[..floor_char_boundary(line, byte)]);
    let ranges: Vec<(usize, usize)> = byte_ranges
        .iter()
        .map(|&(start, end)| (to_units(start), to_units(end.min(line.len()))))
        .collect();
    let first = ranges.first().map_or(0, |&(start, _)| start);
    let units: Vec<u16> = line.encode_utf16().collect();

    let (from, to) = if units.len() <= MAX_PREVIEW_UNITS {
        (0, units.len())
    } else {
        let from = first.saturating_sub(PREVIEW_LEAD_UNITS);
        (from, (from + MAX_PREVIEW_UNITS).min(units.len()))
    };
    // Don't cut a surrogate pair in half.
    let from = if from > 0 && is_low_surrogate(units.get(from)) {
        from - 1
    } else {
        from
    };
    let to = if to < units.len() && is_low_surrogate(units.get(to)) {
        to + 1
    } else {
        to
    };
    let preview = String::from_utf16_lossy(&units[from..to]);
    let spans = ranges
        .iter()
        .filter(|&&(start, end)| end > from && start < to)
        .map(|&(start, end)| Span {
            start: clamp_u32(start.max(from) - from),
            end: clamp_u32(end.min(to) - from),
        })
        .collect();
    LineMatch {
        line: line_number,
        column: clamp_u32(first + 1),
        preview,
        spans,
    }
}

fn is_low_surrogate(unit: Option<&u16>) -> bool {
    unit.is_some_and(|unit| (0xDC00..0xE000).contains(unit))
}

fn floor_char_boundary(text: &str, byte: usize) -> usize {
    let mut byte = byte.min(text.len());
    while !text.is_char_boundary(byte) {
        byte -= 1;
    }
    byte
}

fn clamp_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_and_columns_count_utf16_units() {
        // "é" is 2 bytes and 1 unit; "😀" is 4 bytes and 2 units.
        let line = "é😀 needle\n";
        let start = line.find("needle").expect("needle");
        let found = line_match(3, line, &[(start, start + 6)]);
        assert_eq!(found.line, 3);
        assert_eq!(found.preview, "é😀 needle");
        assert_eq!(found.column, 5);
        assert_eq!(found.spans, vec![Span { start: 4, end: 10 }]);
    }

    #[test]
    fn long_lines_are_cut_around_the_first_match() {
        let line = format!("{}needle{}", "a".repeat(500), "b".repeat(500));
        let found = line_match(1, &line, &[(500, 506)]);
        assert_eq!(utf16_len(&found.preview), MAX_PREVIEW_UNITS);
        assert_eq!(found.column, 501);
        let lead = u32::try_from(PREVIEW_LEAD_UNITS).expect("small");
        assert_eq!(
            found.spans,
            vec![Span {
                start: lead,
                end: lead + 6
            }]
        );
    }

    #[test]
    fn a_cut_never_splits_a_surrogate_pair() {
        // The "x" puts the first match at an odd unit, so both cut points
        // land in the middle of a pair.
        let line = format!("{}xneedle{}", "😀".repeat(300), "😀".repeat(300));
        let start = line.find("needle").expect("needle");
        let found = line_match(1, &line, &[(start, start + 6)]);
        assert!(!found.preview.contains('\u{FFFD}'));
        assert!(found.preview.contains("needle"));
        let needle = found.spans.first().expect("a span");
        let units: Vec<u16> = found.preview.encode_utf16().collect();
        let start = usize::try_from(needle.start).expect("small");
        assert_eq!(String::from_utf16_lossy(&units[start..start + 6]), "needle");
    }

    #[test]
    fn cancelling_one_clone_cancels_all() {
        let flag = CancelFlag::new();
        let copy = flag.clone();
        assert!(!copy.is_cancelled());
        flag.cancel();
        assert!(copy.is_cancelled());
    }
}
