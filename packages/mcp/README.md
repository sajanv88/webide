# @repo/mcp

**Status:** Milestone 7.

## Purpose

AI Agents: the settings of the MCP server that lets agents work in the open
folder (`ide-mcp`), and what they've done, through the `AgentPort` the app
gives it.

- `createAgentStore(port, bus)`: the server's status (on, edits, port, its
  address while it runs, the token, why it isn't running) and the last 100
  tool calls, heard as `agent.call` on the bus.
- `AgentsDialog`: File → AI Agents… (`agents.open`, also in the palette).
  - Switches: "Let agents use the open folder", "Allow agents to edit files".
    They apply at once; a failure is shown, and the switch shows the state
    the server really has.
  - The port, applied with "Use this port".
  - While it runs: Claude Code's command, the token (hidden until shown),
    and an `mcpServers` entry for other clients. Each has a copy button,
    which says when it's copied. "New token" refuses agents with the old
    one from then on.
  - What agents did, newest last; failures are marked.
- `AgentStatusItem`: in the status bar while agents are on: "Agents", with
  what they may do and how many calls they've made, or that the server isn't
  running. Pressing it opens the dialog.
- `claudeCommand(url, token)`, `clientConfig(url, token)`: the setup text.

## Public API

- `createAgentStore`, types `AgentStore`, `AgentState`, `AgentActivity`,
  `MAX_ACTIVITY`; `claudeCommand`, `clientConfig`.
- `AgentsDialog` with `{ store, registry, copy? }` (`copy` puts text on the
  clipboard; the browser's by default).
- `AgentStatusItem` with `{ store, registry }`.

## May depend on

`@repo/contracts` (`AgentPort`, the DTOs, `AppEvents`), `@repo/kernel`,
`react`, `@nuvui/react`, `@nuvui/icons`. Never another feature package,
`@repo/ipc` or `@tauri-apps/*`.
