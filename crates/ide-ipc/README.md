# ide-ipc

**Status:** Milestone 7.

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

| Command                  | Args                                     | Output                                                  |
| ------------------------ | ---------------------------------------- | ------------------------------------------------------- |
| `app_info`               | none                                     | `AppInfoDto` (name and version from tauri.conf.json, `os`) |
| `pick_workspace`         | none                                     | `WorkspaceDto \| null`, after the native folder picker  |
| `open_recent_workspace`  | `{ path }` (from `recent_workspaces`)    | `WorkspaceDto`                                          |
| `recent_workspaces`      | none                                     | `RecentWorkspaceDto[]`                                  |
| `current_workspace`      | none                                     | `WorkspaceDto \| null`                                  |
| `list_directory`         | `{ workspaceId, path }`                  | `DirListingDto`                                         |
| `read_file`              | `{ workspaceId, path }`                  | `ReadFileDto` (switch on `kind`)                        |
| `write_file`             | `{ workspaceId, path, text }`            | `FileStampDto`                                          |
| `get_settings`           | none                                     | `SettingsDto`                                           |
| `update_settings`        | `SettingsPatchDto`                       | `SettingsDto`                                           |
| `search_text`            | `SearchTextArgs`                         | `TextSearchDto` (a newer search cancels this one)       |
| `list_files`             | `{ workspaceId }`                        | `FileListDto`, for quick open                           |
| `git_state`              | `{ workspaceId }`                        | `GitStateDto` (switch on `kind`)                        |
| `git_diff`               | `{ workspaceId, path, side }`            | `GitDiffDto` (switch on `kind`)                         |
| `git_stage`              | `{ workspaceId, paths }`                 | nothing                                                 |
| `git_unstage`            | `{ workspaceId, paths }`                 | nothing                                                 |
| `git_discard`            | `{ workspaceId, paths }`                 | nothing                                                 |
| `git_commit`             | `{ workspaceId, message }`               | nothing                                                 |
| `git_branches`           | `{ workspaceId }`                        | `GitBranchDto[]`                                        |
| `git_switch_branch`      | `{ workspaceId, name, create }`          | nothing                                                 |
| `git_init`               | `{ workspaceId }`                        | nothing                                                 |
| `terminal_open`          | `{ workspaceId, cols, rows, channel }`   | `TerminalDto`; `TerminalEventDto`s on the channel       |
| `terminal_write`         | `{ id, data }`                           | nothing                                                 |
| `terminal_resize`        | `{ id, cols, rows }`                     | nothing                                                 |
| `terminal_close`         | `{ id }`                                 | nothing; ends the shell                                 |
| `lsp_open`, `lsp_change` | `{ workspaceId, path, text }`            | nothing                                                 |
| `lsp_close`              | `{ workspaceId, path }`                  | nothing                                                 |
| `lsp_problems`           | `{ workspaceId, path }`                  | `ProblemDto[]`                                          |
| `lsp_completions`        | `{ workspaceId, path, line, character }` | `CompletionsDto`                                        |
| `lsp_hover`              | `{ workspaceId, path, line, character }` | `HoverDto \| null`                                      |
| `lsp_definition`         | `{ workspaceId, path, line, character }` | `DefinitionDto[]` (switch on `kind`)                    |
| `lsp_resolve_completion` | `{ workspaceId, path, resolve }`         | `ResolvedCompletionDto`: the import a pick adds         |
| `lsp_fixes`              | `{ workspaceId, path, start, end }`      | `CodeActionDto[]`, each with its edits to this file     |
| `lsp_source_action`      | `{ workspaceId, path, action }`          | `TextEditDto[] \| null` (`organizeImports`, `fixAll`…)  |
| `lsp_restart`            | `{ workspaceId }`                        | nothing; it starts again when next needed               |
| `debug_start`            | `{ workspaceId, path, breakpoints }`     | nothing; ends the program already running, if any       |
| `debug_stop`             | `{ workspaceId }`                        | nothing; ends the program                               |
| `debug_breakpoints`      | `{ workspaceId, path, lines }`           | `BreakpointDto[]`, as the debugger placed them          |
| `debug_resume`           | `{ workspaceId, threadId, step? }`       | nothing; continues, or steps `over`, `into` or `out`    |
| `debug_pause`            | `{ workspaceId, threadId }`              | nothing                                                 |
| `debug_stack`            | `{ workspaceId, threadId }`              | `StackFrameDto[]`                                       |
| `debug_scopes`           | `{ workspaceId, reference }` (a frame)   | `ScopeDto[]`                                            |
| `debug_variables`        | `{ workspaceId, reference }`             | `VariableDto[]`                                         |
| `debug_evaluate`         | `{ workspaceId, expression, frameId? }`  | the value, as text                                      |
| `agent_status`           | none                                     | `AgentStatusDto`                                        |
| `agent_configure`        | `{ enabled, allowEdits, port }`          | `AgentStatusDto`; saved, and the server restarted       |
| `agent_new_token`        | none                                     | `AgentStatusDto`; agents with the old token refused     |
| `assistant_status`       | none                                     | `AssistantStatusDto` (the setup, whether a key is kept) |
| `assistant_save`         | `{ setup, secret? }`                     | `AssistantStatusDto`; the key to the credential store   |
| `assistant_forget`       | none                                     | `AssistantStatusDto`; the setup and every key removed   |
| `assistant_models`       | `{ setup, secret? }`                     | `ModelInfoDto[]`, from the provider                     |
| `assistant_check`        | `{ setup, secret? }`                     | the model's one-word answer                             |
| `assistant_send`         | `{ conversationId, text, activePath? }`  | nothing; the reply comes as `assistant` events          |
| `assistant_stop`         | `{ conversationId }`                     | nothing; a waiting change is rejected                   |
| `assistant_decide`       | `{ conversationId, callId, accept }`     | nothing; an accepted change is written                  |
| `assistant_clear`        | `{ conversationId }`                     | nothing; the conversation is forgotten                  |
| `subscribe_events`       | `{ channel }` (a Tauri `Channel`)        | nothing; `BackendEventDto`s arrive on the channel       |
| `set_close_guard`        | `{ unsaved }`                            | nothing                                                 |
| `close_window`           | none                                     | nothing; closes the calling window without asking       |
| `window_minimize`        | none                                     | nothing                                                 |
| `window_toggle_maximize` | none                                     | nothing; maximizes the window, or restores it           |
| `window_drag`            | none                                     | nothing; the window follows the pointer (a press on the title bar) |
| `window_close`           | none                                     | nothing; closes as the system button would (asks first with unsaved changes) |
| `window_state`           | none                                     | `WindowStateDto` (`maximized`, `fullscreen`)            |

## Events (backend → webview)

Sent over the `Channel` a page passes to `subscribe_events`, so the webview
needs no event permission and hears only what we send. One channel per page:
a reloaded page subscribes again, which replaces it, and a page that starts
loading drops the old one.

- `filesChanged { workspaceId, changes, rescan }`: from the watch on the open
  workspace (`ide-watcher`), started when a workspace opens and stopped when
  another does.
- `watchFailed { workspaceId, message }`.
- `languageStatus { workspaceId, status }`: the workspace's language server is
  `starting`, `ready`, or `failed` (with why).
- `debug { workspaceId, event }`: the program printed, stopped, continued or
  ended (`DebugEventDto`, switch on `kind`). The program is ended when another
  workspace opens and when the page reloads.
- `windowState { state }`: the window was maximized or restored, or went
  full screen or back (sent only when that changes). The page draws the
  title bar (no system decorations on Windows and Linux; on macOS the
  system's buttons over the page's bar), so it follows this.
- `agentCall { call }`: an AI agent used a tool of the MCP server
  (`AgentCallDto`: the tool, what it was about, why it failed).
- `assistant { event }`: what a conversation with the assistant reports
  (`AssistantEventDto`, switch on `kind`: `text`, `tool`, `proposal`,
  `proposalDecided`, `done`, `failed`). The assistant's tools are the MCP
  server's, on the folder open now; a key never comes back to the webview.

## The MCP server

Started at launch when the person turned it on, and restarted whenever its
settings or token change (stopped first, so the new one can take the same
port). It answers about whichever workspace is open at the time; a port
that's taken is reported in `agent_status`, not as a failed command.

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
- A terminal runs the person's default shell in the open workspace's folder:
  the webview can't choose the program, its arguments or its folder. At most
  10 at once; a write is at most 1 MiB. Each belongs to the workspace it was
  opened in: opening another workspace, or the page starting again, ends
  them, and one still starting when the workspace changes is refused.
- The language server starts when a file it handles is first opened, once per
  workspace; one that couldn't start isn't tried again until the workspace
  opens again or `lsp_restart`. Another workspace, or the page starting
  again, stops it. Files it doesn't handle get empty answers.
- Each command has its own `allow-*` permission. The main window's capability
  grants the `ipc-commands` set and nothing else.

## Public API

- `run(context, services)`, with `Services { fs, workspace, watcher, search, git, terminals,
  languages }` and `AppPaths` (`config_dir`, `resource_dir`).
- `CONFIG_DIR_VAR` (`WEBIDE_CONFIG_DIR`): set to an absolute folder, the app
  keeps its settings, recent folders and the rest there instead of its own
  place (a separate profile, to try a build without touching yours);
  `config_dir_override` reads it.
- `COMMAND_NAMES`, `IpcError`, `IpcErrorCode`, `MAIN_WINDOW`.

## May depend on

`ide-core`, `tauri`, `tauri-plugin-dialog` (used from Rust only), `serde`,
`thiserror`, `ts-rs`. Never a feature crate. Checked by `cargo xtask check-boundaries`.
