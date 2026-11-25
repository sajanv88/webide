# ide-core

**Status:** Milestone 7.

## Purpose

Pure domain types, errors and the port traits. No Tauri, no IO, no async
runtime: everything here is data and pure functions.

## Public API

- `RelPath`: a validated workspace-relative path (`/`-separated, no `..`, no
  absolute or drive prefix, valid names for the OS via `NameRules`). It's the
  first path guard, applied before any IO.
- `PathError`, `IdeError` (`IdeError::from_io` maps `std::io::Error` kinds;
  `InvalidQuery` for a search that doesn't parse; `Git` for git's refusals;
  `InvalidArgument`).
- `WorkspaceRoot`, `RecentWorkspace`, `workspace::display_path` (drops the `\\?\` prefix).
- `fs`: `FileNode`, `DirListing`, `ReadOutcome`, `ReadLimits`, `FileStamp`,
  `LineEnding`, `ReadOnlyReason`, `sort_nodes`, `detect_line_ending`,
  `looks_binary`, `TEMP_FILE_PREFIX`, `MAX_DIR_ENTRIES`.
- `Settings`, `SettingsPatch`, `Theme`, `EditorSettings` (`indent_guides`,
  `ruler_column`, `Whitespace`, `LineHighlight`), with `Settings::apply`
  validating limits (the ruler from column 10 to 500).
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
- `git`: `GitState` (unavailable, not a repository, or a `GitStatus`: the
  `GitHead` and the changed `GitFileStatus`es, each with a staged and an
  unstaged `GitChange`), `DiffSide`, `DiffTexts` (text, binary, too large),
  `GitBranch`, `valid_branch_name`, `MAX_STATUS_FILES` (5,000).
- `fs::is_git_internal` (a path in a `.git` folder) and
  `ReadOnlyReason::GitInternal`.
- `terminal`: `TerminalSize` (2–1,000 columns, 1–500 rows), `TerminalEvent`
  (output as text, or the exit), `TerminalSink`, `Utf8Stream` (bytes to text
  without splitting a character), `MAX_TERMINALS` (10), `MAX_WRITE_BYTES`
  (1 MiB), and the ports `TerminalHost` and `TerminalSession`.
- `language`: `TextPosition` and `TextRange` (0-based lines, UTF-16
  columns), `Problem` and `Severity`, `Completion`/`Completions` and
  `CompletionKind`, `HoverText`, `Definition` (in the workspace, or outside
  it), `LanguageStatus`, `LanguageSink`, `script_language` (a JavaScript or
  TypeScript file's LSP language id), and the ports `LanguageServers` and
  `LanguageServer`.
- `debug`: `StackFrame` (its `FrameSource`: in the workspace, or outside it),
  `Scope`, `Variable`, `Breakpoint`, `DebugEvent` (output, stopped,
  continued, terminated), `DebugSink`, `Step` (over, into, out), and the ports
  `DebugAdapters` and `DebugSession`.
- `agent`: `AgentConfig` (on, edits allowed, the port: 7341 by default, at
  least 1024), `AgentCall` and `AgentSink` (what an agent did), and the ports
  `AgentHost` (the configuration, the token, serving), `AgentServer` (running;
  dropping it stops it) and `AgentWorkbench` (what the server reads from the
  app: the open folder, the read limits).
- Ports: `WorkspaceFs`, `WorkspaceStore`, `FsWatcher`, `WorkspaceSearch`,
  `GitClient`.

There's no `DocumentId` here: the backend identifies files by `RelPath`, and
only the frontend's tabs need document ids (`@repo/tabs`).

## May depend on

`thiserror` only. No other workspace crate, no Tauri, no async runtime.
Checked by `cargo xtask check-boundaries`.
