//! Find in files and the file list for quick open, on ripgrep's crates.
//!
//! Walks the workspace with `ignore` (so `.gitignore`, `.ignore` and
//! `.git/info/exclude` apply, in folders without git too) and searches each
//! file with `grep-searcher`, on every core. Never follows symlinks, never
//! reads ignore files above the workspace root, and always skips `.git`.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

use grep_matcher::Matcher;
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkMatch};
use ide_core::search::{MAX_LISTED_FILES, MAX_MATCHES, line_match};
use ide_core::{
    CancelFlag, FileList, FileMatches, IdeError, LineMatch, RelPath, TextQuery, TextSearchOutcome,
    WorkspaceRoot, WorkspaceSearch,
};
use ignore::overrides::{Override, OverrideBuilder};
use ignore::{DirEntry, WalkBuilder, WalkState};

#[cfg(test)]
mod tests;

/// Searches the local file system.
#[derive(Debug, Clone, Copy, Default)]
pub struct LocalSearch;

impl LocalSearch {
    pub fn new() -> Self {
        Self
    }
}

impl WorkspaceSearch for LocalSearch {
    fn search_text(
        &self,
        root: &WorkspaceRoot,
        query: &TextQuery,
        cancel: &CancelFlag,
    ) -> Result<TextSearchOutcome, IdeError> {
        if query.pattern.is_empty() {
            return Ok(TextSearchOutcome::default());
        }
        let matcher = matcher(query)?;
        let root = canonical_root(root)?;
        let include = globs(&root, &query.include, false)?;
        let exclude = globs(&root, &query.exclude, true)?;

        // Lines found so far, by every thread. Each searches a file for what's
        // left, so together they may overshoot; the result is cut below.
        let taken = AtomicUsize::new(0);
        let found = Mutex::new(Vec::new());
        walker(&root, exclude).build_parallel().run(|| {
            let mut searcher = SearcherBuilder::new()
                .line_number(true)
                .binary_detection(BinaryDetection::quit(0))
                .build();
            let (root, matcher, include, taken, found) =
                (&root, &matcher, &include, &taken, &found);
            Box::new(move |entry| {
                let left = MAX_MATCHES.saturating_sub(taken.load(Ordering::Relaxed));
                if cancel.is_cancelled() || left == 0 {
                    return WalkState::Quit;
                }
                // An entry that can't be read (permissions, gone mid-walk) is skipped.
                let Ok(entry) = entry else {
                    return WalkState::Continue;
                };
                let Some(path) = file_path(root, &entry) else {
                    return WalkState::Continue;
                };
                if let Some(include) = include
                    && !include.matched(entry.path(), false).is_whitelist()
                {
                    return WalkState::Continue;
                }
                if entry
                    .metadata()
                    .map_or(true, |meta| meta.len() > query.max_file_bytes)
                {
                    return WalkState::Continue;
                }
                let mut sink = Collect {
                    matcher,
                    lines: Vec::new(),
                    budget: left,
                    cancel,
                };
                // A file that can't be read (gone, locked, not text) is skipped.
                if searcher
                    .search_path(matcher, entry.path(), &mut sink)
                    .is_err()
                    || sink.lines.is_empty()
                {
                    return WalkState::Continue;
                }
                taken.fetch_add(sink.lines.len(), Ordering::Relaxed);
                found
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(FileMatches {
                        path,
                        lines: sink.lines,
                    });
                WalkState::Continue
            })
        });

        let mut files = found.into_inner().unwrap_or_else(PoisonError::into_inner);
        sort_by_tree(&mut files);
        // Out of budget: there may be more than what was collected. Keep the
        // first lines, in path order.
        let truncated = taken.load(Ordering::Relaxed) >= MAX_MATCHES;
        if truncated {
            let mut left = MAX_MATCHES;
            files.retain_mut(|file| {
                file.lines.truncate(left);
                left -= file.lines.len();
                !file.lines.is_empty()
            });
        }
        Ok(TextSearchOutcome {
            files,
            truncated,
            cancelled: cancel.is_cancelled(),
        })
    }

    fn list_files(&self, root: &WorkspaceRoot, cancel: &CancelFlag) -> Result<FileList, IdeError> {
        let root = canonical_root(root)?;
        let mut list = FileList::default();
        let entries = walker(&root, None)
            .sort_by_file_path(Path::cmp)
            .build()
            // An entry that can't be read (permissions, gone mid-walk) is skipped.
            .filter_map(Result::ok);
        for entry in entries {
            if cancel.is_cancelled() {
                break;
            }
            let Some(path) = file_path(&root, &entry) else {
                continue;
            };
            if list.files.len() == MAX_LISTED_FILES {
                list.truncated = true;
                break;
            }
            list.files.push(path);
        }
        Ok(list)
    }
}

/// The root as it is now on disk, so a root that moved or became a symlink
/// can't widen what's walked.
fn canonical_root(root: &WorkspaceRoot) -> Result<PathBuf, IdeError> {
    std::fs::canonicalize(root.path()).map_err(|error| IdeError::from_io(&error))
}

fn matcher(query: &TextQuery) -> Result<RegexMatcher, IdeError> {
    RegexMatcherBuilder::new()
        .case_insensitive(!query.match_case)
        .word(query.whole_word)
        .fixed_strings(!query.is_regex)
        .line_terminator(Some(b'\n'))
        .build(&query.pattern)
        .map_err(|error| IdeError::InvalidQuery(error.to_string()))
}

/// Gitignore-style globs. `negate` makes each one a glob to leave out.
fn globs(root: &Path, patterns: &[String], negate: bool) -> Result<Option<Override>, IdeError> {
    let patterns: Vec<&str> = patterns
        .iter()
        .map(|pattern| pattern.trim())
        .filter(|pattern| !pattern.is_empty())
        .collect();
    if patterns.is_empty() {
        return Ok(None);
    }
    let mut builder = OverrideBuilder::new(root);
    for pattern in patterns {
        let glob = if negate {
            format!("!{pattern}")
        } else {
            pattern.to_owned()
        };
        builder
            .add(&glob)
            .map_err(|error| IdeError::InvalidQuery(error.to_string()))?;
    }
    builder
        .build()
        .map(Some)
        .map_err(|error| IdeError::InvalidQuery(error.to_string()))
}

/// Walks every entry under `root` that ignore files and `exclude` leave in.
fn walker(root: &Path, exclude: Option<Override>) -> WalkBuilder {
    let mut builder = WalkBuilder::new(root);
    builder
        // Dotfiles (.github, .env.example) are searched unless ignored.
        .hidden(false)
        .parents(false)
        .git_global(false)
        .require_git(false)
        .follow_links(false)
        .filter_entry(move |entry| {
            if entry.depth() > 0 && entry.file_name() == ".git" {
                return false;
            }
            let is_dir = entry.file_type().is_some_and(|kind| kind.is_dir());
            exclude
                .as_ref()
                .is_none_or(|exclude| !exclude.matched(entry.path(), is_dir).is_ignore())
        });
    builder
}

/// The order a sorted walk gives: by name, each folder's contents where the
/// folder is (`a/b.ts` before `a.ts`), which is segment by segment.
fn sort_by_tree(files: &mut [FileMatches]) {
    files.sort_by(|a, b| a.path.segments().cmp(b.path.segments()));
}

/// The workspace-relative path of a regular file (not a symlink or a folder).
fn file_path(root: &Path, entry: &DirEntry) -> Option<RelPath> {
    if !entry.file_type()?.is_file() {
        return None;
    }
    let relative = entry.path().strip_prefix(root).ok()?;
    let segments: Option<Vec<&str>> = relative
        .components()
        .map(|component| component.as_os_str().to_str())
        .collect();
    RelPath::parse(&segments?.join("/")).ok()
}

/// Collects the matching lines of one file, up to the budget left.
struct Collect<'a> {
    matcher: &'a RegexMatcher,
    lines: Vec<LineMatch>,
    budget: usize,
    cancel: &'a CancelFlag,
}

impl Sink for Collect<'_> {
    type Error = io::Error;

    fn matched(&mut self, _searcher: &Searcher, found: &SinkMatch<'_>) -> Result<bool, io::Error> {
        if self.budget == 0 || self.cancel.is_cancelled() {
            return Ok(false);
        }
        let text = String::from_utf8_lossy(found.bytes());
        let mut ranges = Vec::new();
        self.matcher
            .find_iter(text.as_bytes(), |span| {
                if span.start() < span.end() {
                    ranges.push((span.start(), span.end()));
                }
                true
            })
            .map_err(|error| io::Error::other(error.to_string()))?;
        let line = u32::try_from(found.line_number().unwrap_or(0)).unwrap_or(u32::MAX);
        self.lines.push(line_match(line, &text, &ranges));
        self.budget -= 1;
        Ok(self.budget > 0)
    }
}
