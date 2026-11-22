# ide-search

**Status:** Milestone 2.

## Purpose

Find in files, and the list of files for quick open, on ripgrep's own crates.

- The walk is `ignore`'s: `.gitignore`, `.ignore` and `.git/info/exclude`
  apply, in folders without git too. The `.git` folder is always skipped.
  Dotfiles (`.github`, `.env.example`) are searched unless ignored. Ignore
  files above the workspace root and the global git excludes aren't read, so
  nothing outside the workspace decides what's in it.
- Symlinks are never followed, so the search can't leave the workspace.
- Files are searched with `grep-searcher`, on every core: line by line, binary files
  (a NUL byte) skipped, files over the size limit skipped (the app passes the
  "don't open above" setting).
- The pattern is text as written, or a regular expression (Rust's `regex`
  syntax); case-insensitive unless asked; whole words optionally. A pattern
  or glob that doesn't parse is `IdeError::InvalidQuery`.
- Include and exclude globs use gitignore syntax (`*.ts`, `src/`, `docs/**`).
  An include glob narrows what's searched but doesn't bring back an ignored
  file.
- Results come in walk order: sorted by name, each folder's contents where
  the folder is. Each line gives its number, the column of its first match
  and every match as a span, in UTF-16 units (what JavaScript and the editor
  count). A long line is cut to 240 units around its first match.
- One search stops at 10,000 matching lines (`truncated`), keeping the first
  ones in path order, and checks its `CancelFlag` between files and lines
  (`cancelled`). The file list stops at 50,000 files.

### Known limits

- When a search hits its limit, which files it had reached varies from run
  to run (they're searched in parallel), so a truncated result can differ.
- A match can't span lines.
- No replace yet.

## Public API

- `LocalSearch`, which implements `ide_core::WorkspaceSearch`
  (`search_text`, `list_files`).

## May depend on

`ide-core`, `ignore`, `grep-searcher`, `grep-regex`, `grep-matcher`. Never
another workspace crate, Tauri or an async runtime. Checked by
`cargo xtask check-boundaries`.
