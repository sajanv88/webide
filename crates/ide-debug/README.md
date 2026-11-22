# ide-debug

**Status:** Milestone 6.

## Purpose

Debugging JavaScript and TypeScript programs with Node, over the Debug
Adapter Protocol, through vscode-js-debug's DAP server. The server (1.140.1,
checked by its SHA-256) is fetched and staged by
`apps/desktop/scripts/stage-js-debug.mjs` and bundled with the app; it runs on
the person's own `node`.

- **One program at a time** per workspace: a file in the workspace, run with
  its folder as the working directory. The webview names only the file and
  the breakpoints; never the program, its arguments or the adapter.
- **The server** listens on a free port on 127.0.0.1, under a small wrapper
  that ends it when its stdin closes. (`node` may be a shim, such as pnpm's
  or Volta's, that starts the real one as a child: killing the shim would
  leave the server running.)
- **Two sessions, one to the caller.** js-debug's first session launches the
  program and sends its output; it then asks (`startDebugging`) for a second
  connection, where the program's threads, breakpoints, stack and variables
  are. Both are handled here, behind one `DebugSession`. Breakpoints are set
  on the second session before the program runs past them.
- **Paths** the debugger names are workspace paths when they're inside the
  folder (separators and a drive letter's case don't matter), and are shown
  by name otherwise.
- **Stepping** skips Node's own code (`<node_internals>`).
- **Limits:** 30 seconds to start, 10 to answer a request, 64 MiB per message.
- **Ending:** dropping the session disconnects (which ends the program),
  closes both connections, closes the server's stdin, and waits up to three
  seconds before killing it.

### Known limits

- Node programs only: no browser, no attaching to a running process, no
  launch configurations (arguments, environment).
- TypeScript runs as Node runs it: Node 22.18+ strips types; older Nodes
  can't run a `.ts` file.
- No conditional breakpoints, logpoints or watch expressions yet.

## Public API

- `NodeDebugger::new(server_js)` (implements `ide_core::DebugAdapters`).
- `find_node()`, `workspace_path(root, path)`.
- `DapConnection`, `DapError`, `read_message`, `write_message`: the protocol,
  over any stream.

## May depend on

`ide-core`, `serde_json`. Never another workspace crate, Tauri or an async
runtime. Checked by `cargo xtask check-boundaries`.
