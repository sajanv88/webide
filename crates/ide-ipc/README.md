# ide-ipc

**Status:** Milestone 3.

## Purpose

The only crate that depends on Tauri. It builds the app, registers the
commands, maps domain types to DTOs, and exports those DTOs to TypeScript with
ts-rs.

- `src/commands.rs` holds the one command list. It drives the invoke handler,
  the generated `CommandMap`, and the generated permissions, and it checks at
  compile time that each handler matches its declared types.
- `src/handlers.rs` has what each command does, as plain functions with no
  Tauri, tested with fake ports.
- `src/events.rs` holds the page's event channel and the close guard.
- `src/dto.rs` and `src/error.rs` define what crosses the boundary.
  `IpcErrorCode` is stable, and every `IdeError` maps to one.
- `src/export.rs` is run by `pnpm contracts:generate`. It writes
  `packages/contracts/src/generated/*`, `apps/desktop/src-tauri/ipc_commands.rs`
  and `apps/desktop/src-tauri/permissions/ipc-commands.toml`.

## Commands

| Command                 | Args                                  | Output                                                 |
| ----------------------- | ------------------------------------- | ------------------------------------------------------ |
| `app_info`              | none                                  | `AppInfoDto` (name and version from tauri.conf.json)   |
| `pick_workspace`        | none                                  | `WorkspaceDto \| null`, after the native folder picker |
| `open_recent_workspace` | `{ path }` (from `recent_workspaces`) | `WorkspaceDto`                                         |
| `recent_workspaces`     | none                                  | `RecentWorkspaceDto[]`                                 |
| `current_workspace`     | none                                  | `WorkspaceDto \| null`                                 |
| `list_directory`        | `{ workspaceId, path }`               | `DirListingDto`                                        |
| `read_file`             | `{ workspaceId, path }`               | `ReadFileDto` (switch on `kind`)                       |
| `write_file`            | `{ workspaceId, path, text }`         | `FileStampDto`                                         |
| `get_settings`          | none                                  | `SettingsDto`                                          |
| `update_settings`       | `SettingsPatchDto`                    | `SettingsDto`                                          |
| `search_text`           | `SearchTextArgs`                      | `TextSearchDto` (a newer search cancels this one)      |
| `list_files`            | `{ workspaceId }`                     | `FileListDto`, for quick open                          |
| `git_state`             | `{ workspaceId }`                     | `GitStateDto` (switch on `kind`)                       |
| `git_diff`              | `{ workspaceId, path, side }`         | `GitDiffDto` (switch on `kind`)                        |
| `git_stage`             | `{ workspaceId, paths }`              | nothing                                                |
| `git_unstage`           | `{ workspaceId, paths }`              | nothing                                                |
| `git_discard`           | `{ workspaceId, paths }`              | nothing                                                |
| `git_commit`            | `{ workspaceId, message }`            | nothing                                                |
| `git_branches`          | `{ workspaceId }`                     | `GitBranchDto[]`                                       |
| `git_switch_branch`     | `{ workspaceId, name, create }`       | nothing                                                |
| `git_init`              | `{ workspaceId }`                     | nothing                                                |
| `subscribe_events`      | `{ channel }` (a Tauri `Channel`)     | nothing; `BackendEventDto`s arrive on the channel      |
| `set_close_guard`       | `{ unsaved }`                         | nothing                                                |
| `close_window`          | none                                  | nothing; closes the calling window without asking      |

## Events (backend → webview)

Sent over the `Channel` a page passes to `subscribe_events`, so the webview
needs no event permission and hears only what we send. One channel per page:
a reloaded page subscribes again, which replaces it, and a page that starts
loading drops the old one.

- `filesChanged { workspaceId, changes, rescan }`: from the watch on the open
  workspace (`ide-watcher`), started when a workspace opens and stopped when
  another does.
- `watchFailed { workspaceId, message }`.
- `closeRequested`: the window (or, on macOS, the app) is closing while the
  page has said `set_close_guard({ unsaved: true })`. The close is held; the
  page asks the person, then calls `close_window`. With nothing unsaved, or no
  page listening, the window closes as usual.

The rules that keep the webview contained:

- The webview can't choose a workspace root. A root is opened only from the
  native picker, or from the recent list that the picker fills.
- File commands carry the `workspaceId` of the open workspace. A request meant
  for another one fails with `workspaceChanged`.
- Paths are validated (`RelPath`) and contained (`ide-fs`) before any IO.
- Search walks only the open workspace's root (`ide-search` never follows
  symlinks) and skips files over the "don't open above" size. One search runs
  at a time: a new one, or opening another workspace, cancels it.
- Git commands take workspace-relative paths, checked as `RelPath`s before git
  sees them. Git's refusals come back as the `git` code with git's own words.
- Each command has its own `allow-*` permission. The main window's capability
  grants the `ipc-commands` set and nothing else.

## Public API

- `run(context, services)`, with `Services { fs, workspace, watcher, search, git }` and `AppPaths`.
- `COMMAND_NAMES`, `IpcError`, `IpcErrorCode`, `MAIN_WINDOW`.

## May depend on

`ide-core`, `tauri`, `tauri-plugin-dialog` (used from Rust only), `serde`,
`thiserror`, `ts-rs`. Never a feature crate. Checked by `cargo xtask check-boundaries`.
