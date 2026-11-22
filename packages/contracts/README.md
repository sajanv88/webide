# @repo/contracts

**Status:** Milestone 5: IPC types (generated), and the app's command ids, events and ports (hand-written).

## Purpose

The types modules share.

- `src/generated/`: every IPC command and DTO, generated from the Rust DTOs in
  `crates/ide-ipc` with [ts-rs](https://github.com/Aleph-Alpha/ts-rs) by
  `pnpm contracts:generate`. Never written by hand. `pnpm lint` runs
  `cargo xtask check-contracts`, which fails if this folder drifts.
- `src/app.ts`: frontend-only contracts, hand-written because they have no Rust
  side: `AppCommandId`, `AppEvents`, the ports feature packages take, and
  `IpcResult`.

## Public API

Types only (`export type *`):

- `CommandMap`, `CommandName`: every command's `args` (`null` when none) and `output`.
- `IpcError`, `IpcErrorCode`: what a failing command rejects with.
- The DTOs: `WorkspaceDto`, `RecentWorkspaceDto`, `DirListingDto`, `FileNodeDto`,
  `ReadFileDto` (switch on `kind`), `FileStampDto`, `SettingsDto`, `SettingsPatchDto`,
  `TextSearchDto` (`FileMatchesDto`, `LineMatchDto`, `SpanDto`), `FileListDto`,
  `GitStateDto`, `GitHeadDto`, `GitFileDto`, `GitChangeDto`, `GitDiffDto`,
  `GitBranchDto`, `DiffSideDto`, and `GitDiffArgs`, `GitPathsArgs`,
  `GitCommitArgs`, `GitSwitchArgs`, `TerminalDto`, `TerminalEventDto`,
  `TerminalOpenArgs`, `TerminalWriteArgs`, `TerminalResizeArgs`, `TerminalIdArgs`,
  `ProblemDto`, `CompletionsDto`, `HoverDto`, `DefinitionDto`, `LanguageStatusDto`,
  `LspDocumentArgs`, `LspPositionArgs`,
  and the argument types `WorkspacePathArgs`, `WorkspaceArgs`, `WriteFileArgs`,
  `OpenRecentArgs`, `SearchTextArgs`.
- `IpcResult<T>`: `{ ok: true, value } | { ok: false, error: IpcError }`.
- `AppCommandId`: every command id in the app's registry.
- `AppEvents`: `workspace.opened`, `explorer.fileActivated`, `file.openRequested`
  (a path, and optionally where to put the cursor), `workspace.filesChanged`,
  `settings.changed`, `document.opened` / `document.changed` / `document.closed`
  (an open file's text), `language.problems`, `language.status`.
- `EditorProblem`: a problem placed by character offset, and by line.
- `ExplorerPort`: `listDirectory(path)`, bound to the open workspace by the app.
- `DocumentsPort`: `readFile(path)`, `writeFile(path, text)`, the same way, for `@repo/tabs`.
- `SearchPort`: `searchText(query)`, `listFiles()`, the same way, for `@repo/search`.
- `GitPort`: `state()`, `diff(path, side)`, `stage`, `unstage`, `discard`,
  `commit(message)`, `branches()`, `switchBranch(name, create)`, `init()`, for `@repo/git`.
- `TerminalPort`: `open(cols, rows, onEvent)` → `OpenTerminal`, `write`, `resize`, `close`,
  for `@repo/terminal`.
- `LanguagePort`: `open`, `change`, `close`, `problems`, `completions`, `hover`,
  `definition`, `restart`, for `@repo/lsp`.

## May depend on

Nothing. Plain TypeScript types, no runtime code.
