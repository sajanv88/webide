# ide-lsp

**Status:** Milestone 5, and more languages.

## Purpose

Language features over LSP, from servers the app ships:

| Server     | Files                                                                                           | Runs as                                            |
| ---------- | ----------------------------------------------------------------------------------------------- | -------------------------------------------------- |
| TypeScript | `.ts`, `.tsx`, `.mts`, `.cts`, `.js`, `.jsx`, `.mjs`, `.cjs`                                    | TypeScript 7's native `tsc --lsp --stdio`          |
| JSON       | `.json`, `.jsonc` (`tsconfig*.json`, `.vscode/*.json`… as JSON with comments)                   | VS Code's JSON server, on Node                     |
| CSS        | `.css`, `.scss`, `.less`                                                                        | VS Code's CSS server, on Node                      |
| Angular    | `.html` templates, in an Angular project (`angular.json`, or `@angular/core` in `package.json`) | Angular's server, on Node, with its own TypeScript |
| HTML       | `.html`, `.htm` (with the CSS and JavaScript in them), in any other project                     | VS Code's HTML server, on Node                     |
| YAML       | `.yaml`, `.yml`                                                                                 | Red Hat's YAML server, on Node                     |

- **The programs** are the app's own: `tsc` from its `typescript`
  dependency (`apps/desktop/scripts/stage-typescript.mjs`), the others from
  `vscode-langservers-extracted` and `yaml-language-server`, bundled by
  esbuild into one file each (`stage-language-servers.mjs`). Nothing from the
  workspace is run: not its `node_modules`, not its TypeScript.
- **`Servers`** holds a `ServerSpec` per server (its name, program, the
  files it handles, its settings). In a workspace, each server starts the
  first time one of its files opens, and reports its status under its own
  name ("TypeScript", "JSON"). One that can't start isn't tried again until
  the servers are restarted.
- **Angular:** a spec can serve only some workspaces (`applies`): Angular's
  is asked whether the folder is an Angular project, once as it opens, and
  is listed before HTML, which has the `.html` files of every other one. A
  project is Angular's when `angular.json` has `projects`, or
  `package.json` has `@angular/core` among its dependencies (both parsed).
  Its server loads TypeScript and `@angular/language-service` only from the
  copies the app bundles beside it (`--tsProbeLocations`,
  `--ngProbeLocations` name nothing else), never the project's: opening a
  template runs nothing from the workspace, which it only reads (its
  sources and its packages' type declarations). `{workspace}` in a spec's
  `args` is the folder.
- **Trust:** a spec that runs the folder's own code (its configuration,
  plugins or packages) sets `needs_trust`. In a folder the person hasn't
  trusted (`start(root, trusted, status)`), it doesn't start: the first file
  it would have had reports `Restricted` for it, once, and the file goes to
  the next server that handles it, if any.
- **Node:** the JSON, CSS, HTML, YAML and Angular servers run on the person's `node`,
  found when each starts (`FindNode`: the app gives the debugger's
  `find_node`, which also looks where installers put it). Without one they
  report `NeedsNode`, and the app asks for it.
- **Schemas:** JSON and YAML files are checked against SchemaStore schemas
  bundled with the app (`package.json`, `tsconfig.json`, `.eslintrc`,
  `.prettierrc`, `.babelrc`, `nest-cli.json`, GitHub workflows and actions,
  Dependabot, `pnpm-workspace.yaml`…; pinned with their SHA-256 in
  `apps/desktop/scripts/schemas.lock.json`). The JSON server asks the app for
  a SchemaStore address (`vscode/content`), answered from the bundle by file
  name only; the YAML server is given the files. Nothing is downloaded, and
  YAML's SchemaStore catalog is off. A schema a file names for itself at an
  address (JSON's `"$schema"`, YAML's `# yaml-language-server: $schema=`) is
  asked of the app too (the YAML server is told to with
  `yaml/registerContentRequest`), so opening a file never makes a server
  fetch anything; one that isn't bundled is refused.
- **`rpc`:** JSON-RPC 2.0 over the server's stdio, in LSP frames
  (`Content-Length`). Requests have time limits (15 s, 60 s to start) and
  are cancelled (`$/cancelRequest`) past them. The server's own requests
  are answered at once: `workspace/configuration` with its settings by
  section (or the defaults), `vscode/content` with a bundled schema, the
  rest with "fine". Servers that read settings when told get them once
  they've started (`workspace/didChangeConfiguration`). Its log on stderr is
  read and dropped. On Windows it opens no console window.
- **The session:** files are opened, changed (the whole text each time) and
  closed as the editor does; problems are pulled (`textDocument/diagnostic`)
  from a server that offers it, and otherwise taken from what it sends
  unasked (`textDocument/publishDiagnostics`, as YAML does), waiting up to 2 s
  after a change for them; completions (at most 1,000, with where the
  server's edit starts, as a JSON key replaces its quote), hover (Markdown)
  and go to definition (`Location` or `LocationLink`). Positions are UTF-16,
  as the editor counts them.
- **Imports and fixes:** a completion the server has more to say about gets
  a `resolve` token (the list's number and the item's): picking it asks
  `completionItem/resolve` for the import it adds. Only the latest list's
  tokens, for its file, are known. Fixes are the `quickfix` code actions for
  the problems at a place, asked with those problems as the server reports
  them then; source actions are `source.organizeImports`,
  `source.sortImports`, `source.removeUnusedImports` and `source.fixAll`.
  Only edits are made (never a command the server would run), and only
  those that change the file asked about.
- **`uri`:** `file://` URIs to and from paths, `%`-encoded as servers write
  them (the TypeScript server writes `file:///c%3A/…`); a Windows drive
  letter matches in any case. A definition inside the workspace is a
  workspace path (it can be opened); outside it (the language's `lib.*.d.ts`)
  it's named only.
- Dropping a session asks the server to leave (`shutdown`, `exit`) and then
  makes sure. A server that stops by itself reports `Failed`.

### Known limits

- No choosing the project's own TypeScript.
- No formatting by any server.
- Angular covers templates in `.html` files; inline templates (in a
  component's `.ts`) are TypeScript's, without Angular's checks.
- Compose files and Renovate's config have no schema: SchemaStore points to
  their projects' own, which aren't bundled.
- The YAML server's file patterns (`**/.github/workflows/*.yml`) don't match
  a workspace inside a folder whose name starts with a dot.
- No rename, references, refactorings, formatting or signature help yet
  (the server has them). A fix that would change another file isn't offered.
- Problems are asked for again after a change, for the files that are open,
  not for the whole project.

## Public API

- `Servers::new(specs, find_node)` (implements `ide_core::LanguageServers`),
  with `ServerSpec::typescript(program)`, `json(script, schemas)`,
  `css(script)`, `angular(script)`, `html(script)`, `yaml(script, schemas)`;
  `Program` (`Native`, `Node`), `FindNode`, `LanguageId`, `WORKSPACE_ARG`,
  `angular_workspace`, and `json_language`, `css_language`, `html_language`,
  `yaml_language`.
- `RpcClient`, `RpcError`, `OnRequest`, `read_message`, `write_message`;
  `to_uri`, `from_uri`, `relative_to`; `MAX_COMPLETIONS`.

## May depend on

`ide-core`, `serde_json`. Never another workspace crate, Tauri or an async
runtime. Checked by `cargo xtask check-boundaries`.
