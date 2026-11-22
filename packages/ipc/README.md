# @repo/ipc

**Status:** Milestone 2: every command, typed, and the backend's events.

## Purpose

The only package that imports `@tauri-apps/api`. It turns the commands in
`@repo/contracts` into typed functions. Feature packages never import it:
`apps/desktop` passes these functions to them as ports.

## Public API

- `call(name, args?)`: any command from `CommandMap`, with argument and output
  types from Rust. Input is sent as `{ args }`.
- Named wrappers: `appInfo`, `pickWorkspace`, `openRecentWorkspace`,
  `recentWorkspaces`, `currentWorkspace`, `listDirectory`, `readFile`,
  `writeFile`, `getSettings`, `updateSettings`, `searchText`, `listFiles`.
- `subscribeEvents(onEvent)`: hears `BackendEventDto`s (file changes, the
  window closing) over a Tauri `Channel`; resolves to the function that stops.
- `setCloseGuard(unsaved)`, `closeWindow()`: the window asks before closing
  while there are unsaved changes.
- `IpcResult<T>`: `{ ok: true, value }` or `{ ok: false, error: IpcError }`. Calls
  never throw for expected failures.
- `toIpcError(unknown)`: a rejection as an `IpcError`. Anything that isn't one,
  such as an ACL denial or bad arguments, becomes `internal`.
- `getStyleNonce()`: the CSP nonce Tauri put on this page's `<style>` elements,
  for nuvui `ScrollArea`.

## May depend on

`@repo/contracts` and `@tauri-apps/api`. Nothing else in the workspace.
