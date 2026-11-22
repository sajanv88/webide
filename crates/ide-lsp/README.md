# ide-lsp

**Status:** Milestone 5.

## Purpose

Language features over LSP: for now TypeScript and JavaScript (`.ts`, `.tsx`,
`.mts`, `.cts`, `.js`, `.jsx`, `.mjs`, `.cjs`), from the TypeScript server
that ships with the app.

- **The server** is TypeScript 7's native `tsc`, run as `tsc --lsp --stdio`.
  It comes from the app's own `typescript` dependency, staged into the app's
  resources at build time (`apps/desktop/scripts/stage-typescript.mjs`).
  Nothing from the workspace is run: not its `node_modules`, not its
  TypeScript.
- **`rpc`:** JSON-RPC 2.0 over the server's stdio, in LSP frames
  (`Content-Length`). Requests have time limits (15 s, 60 s to start) and
  are cancelled (`$/cancelRequest`) past them. The server's own requests
  (`workspace/configuration`, `client/registerCapability`…) are answered at
  once with defaults: the TypeScript server waits for those answers. Its log
  on stderr is read and dropped. On Windows it opens no console window.
- **The session:** files are opened, changed (the whole text each time) and
  closed as the editor does; problems are pulled (`textDocument/diagnostic`,
  as the TypeScript server wants); completions (at most 1,000), hover
  (Markdown) and go to definition (`Location` or `LocationLink`).
  Positions are UTF-16, as the editor counts them.
- **`uri`:** `file://` URIs to and from paths, `%`-encoded as servers write
  them (the TypeScript server writes `file:///c%3A/…`); a Windows drive
  letter matches in any case. A definition inside the workspace is a
  workspace path (it can be opened); outside it (the language's `lib.*.d.ts`)
  it's named only.
- Dropping a session asks the server to leave (`shutdown`, `exit`) and then
  makes sure. A server that stops by itself reports `Failed`.

### Known limits

- One language: TypeScript's server, for TypeScript and JavaScript. No
  others yet, and no choosing the project's own TypeScript.
- No rename, references, code actions, formatting or signature help yet
  (the server has them).
- Problems are asked for again after a change, for the files that are open,
  not for the whole project.

## Public API

- `TypeScriptServers::new(program)` (implements `ide_core::LanguageServers`).
- `RpcClient`, `RpcError`, `read_message`, `write_message`; `to_uri`,
  `from_uri`, `relative_to`; `MAX_COMPLETIONS`.

## May depend on

`ide-core`, `serde_json`. Never another workspace crate, Tauri or an async
runtime. Checked by `cargo xtask check-boundaries`.
