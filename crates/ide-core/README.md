# ide-core

**Status:** Milestone 2.

## Purpose

Pure domain types, errors and the port traits. No Tauri, no IO, no async
runtime: everything here is data and pure functions.

## Public API

- `RelPath`: a validated workspace-relative path (`/`-separated, no `..`, no
  absolute or drive prefix, valid names for the OS via `NameRules`). It's the
  first path guard, applied before any IO.
- `PathError`, `IdeError` (`IdeError::from_io` maps `std::io::Error` kinds;
  `InvalidQuery` for a search that doesn't parse).
- `WorkspaceRoot`, `RecentWorkspace`, `workspace::display_path` (drops the `\\?\` prefix).
- `fs`: `FileNode`, `DirListing`, `ReadOutcome`, `ReadLimits`, `FileStamp`,
  `LineEnding`, `ReadOnlyReason`, `sort_nodes`, `detect_line_ending`,
  `looks_binary`, `TEMP_FILE_PREFIX`, `MAX_DIR_ENTRIES`.
- `Settings`, `SettingsPatch`, `Theme`, with `Settings::apply` validating limits.
- `watch`: `FsChange` (created, modified, removed, renamed), `WatchEvent`
  (changes, possibly `rescan`; or failed), `WatchSink`, `WatchGuard` (dropping
  it stops the watch), `coalesce` (one change per path; over
  `MAX_CHANGES_PER_BATCH`, a rescan instead).
- `search`: `TextQuery` (pattern, regex, case, whole word, include and
  exclude globs, a file size limit), `TextSearchOutcome` of `FileMatches` of
  `LineMatch` (line, column, a preview cut around the first match, and
  `Span`s, all in UTF-16 units as JavaScript counts), `FileList`,
  `CancelFlag`, `line_match`, and the limits `MAX_MATCHES` (10,000 lines),
  `MAX_LISTED_FILES` (50,000), `MAX_PREVIEW_UNITS`.
- Ports: `WorkspaceFs`, `WorkspaceStore`, `FsWatcher`, `WorkspaceSearch`.

There's no `DocumentId` here: the backend identifies files by `RelPath`, and
only the frontend's tabs need document ids (`@repo/tabs`).

## May depend on

`thiserror` only. No other workspace crate, no Tauri, no async runtime.
Checked by `cargo xtask check-boundaries`.
