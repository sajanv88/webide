# ide-mcp

**Status:** Milestone 7.

## Purpose

An MCP server that lets AI agents (Claude Code, or any client of the Model
Context Protocol) work in the folder open in the IDE. It's built on the
official Rust SDK (`rmcp`, its Streamable HTTP transport), served by hyper
on a tokio runtime of its own thread.

- **Off by default.** The person turns it on in File → AI Agents….
- **On this machine only.** It listens on 127.0.0.1 (port 7341 unless the
  person picks another, from 1024).
- **A token.** Every request must carry `Authorization: Bearer <token>`: 32
  bytes from the OS's randomness (`getrandom`), as hex, compared in constant
  time. The token is kept in the app's config folder (`agent-token`, readable
  only by the person on Unix), so an agent's setup survives restarts; a new
  one can be made at any time, and the old one is refused from then on.
- **Not from web pages.** A request with an `Origin` header (browsers send
  one) is refused, and so is a `Host` that isn't `127.0.0.1` or `localhost`,
  so a site the person visits can't reach the server, even by DNS rebinding.
- **Stateless JSON.** Each request is answered on its own, with plain JSON:
  there are no sessions to keep.
- **Read by default.** The tools: `workspace_info`, `list_directory`,
  `read_file` (from the disk, optionally some lines), `find_files`,
  `search_text`, `git_status`, `git_diff` (unified). With edits allowed,
  `write_file` too; without, it isn't offered, and is refused if called.
  Nothing is ever written in `.git` (a hook written there would run on the
  next commit), and no tool runs a command.
- **Through the app's ports.** Paths go through `RelPath` and the workspace
  ports (`WorkspaceFs`, `WorkspaceSearch`, `GitClient`), so an agent can't
  reach outside the folder, and the size limits from the settings apply.
  Absolute paths inside the folder are accepted, as agents often send them.
- **Seen.** Every call is reported (`AgentSink`): the tool, what it was
  about, and why it failed.

The configuration (on, edits, port) is kept beside the settings, in
`agents.json`.

### Known limits

- Unsaved changes in the IDE's editors aren't visible to agents.
- One workspace: the one open in the window.
- No problems from the language server yet, and no prompts or resources.

## Public API

- `McpHost::new(config_dir, fs, search, git)` (implements
  `ide_core::agent::AgentHost`).
- `unified_diff(path, original, modified)`, `workspace_relative(root, path)`.

## May depend on

`ide-core`, `rmcp`, `tokio`, `hyper`, `hyper-util`, `schemars`, `serde`,
`serde_json`, `getrandom`. Never another workspace crate or Tauri. Checked by
`cargo xtask check-boundaries`.
