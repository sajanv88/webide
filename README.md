# webide

A free, open-source desktop IDE for JavaScript and TypeScript, built with Tauri 2,
Rust, React 19 and nuvui. `webide` is a working name. The app's name lives in one
place: `productName` in [apps/desktop/src-tauri/tauri.conf.json](apps/desktop/src-tauri/tauri.conf.json).

Status and plan: [checklist.md](checklist.md), from [plan.md](plan.md).

## Keys

| Keys (macOS: Cmd for Ctrl)  | Does                      |
| --------------------------- | ------------------------- |
| Ctrl+Shift+P, F1            | Command palette           |
| Ctrl+O                      | Open folder               |
| Ctrl+S / Ctrl+Alt+S         | Save / Save all           |
| Ctrl+W                      | Close editor              |
| Ctrl+PageDown / Ctrl+PageUp | Next / previous editor    |
| Ctrl+P                      | Go to file                |
| Ctrl+Shift+F                | Find in files             |
| Ctrl+Shift+E                | Show the explorer         |
| Ctrl+Shift+G                | Show source control       |
| Ctrl+Shift+D                | Show Run and Debug        |
| F5                          | Start debugging, continue |
| Shift+F5                    | Stop debugging            |
| F9                          | Toggle a breakpoint       |
| F10 / F11 / Shift+F11       | Step over / into / out    |
| Ctrl+`                      | Toggle the terminal       |
| Ctrl+Shift+`                | New terminal              |
| F12, Ctrl+click             | Go to definition          |
| Ctrl+Space                  | Show completions          |
| Ctrl+.                      | Quick fix                 |
| Shift+Alt+O                 | Organize imports          |
| Ctrl+B                      | Show or hide the side bar |
| Ctrl+,                      | Settings                  |

On macOS, Ctrl+PageDown, Ctrl+PageUp, Ctrl+Shift+G and the terminal keys stay
on the Control key. They're one
table, [apps/desktop/src/keybindings.ts](apps/desktop/src/keybindings.ts);
the menus and the palette show them from there.

## Requirements

- Node 24+, pnpm (the version in `packageManager` is fetched by pnpm itself)
- Rust via rustup (the toolchain in `rust-toolchain.toml` is installed on first use)
- The [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS

`pnpm build` (and `pnpm dev`) first copies the TypeScript language server for
your platform, from the app's own `typescript` dependency, and the
vscode-js-debug DAP server (downloaded from its GitHub release once, checked
by its SHA-256, then cached), into `apps/desktop/src-tauri/resources/`, which
the app bundles. It also bundles the JSON, CSS, HTML, YAML and Angular language
servers with esbuild (Angular's with its language service; it loads the
project's own TypeScript), and the SchemaStore schemas they check files against (pinned in
`apps/desktop/scripts/schemas.lock.json`, fetched once and cached). The
debugger and those servers run on your own `node`: the app asks for it, and
offers its installer, when it's missing. Run `pnpm build`
once before `cargo` builds the app on its own (Tauri checks the resources
exist).

## Commands

| Command                                                                                                        | What it does                                                            |
| -------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| `pnpm install`                                                                                                 | Installs JS dependencies (install scripts are denied unless reviewed)   |
| `pnpm dev`                                                                                                     | Runs the desktop app with hot reload (`tauri dev`)                      |
| `pnpm tauri build`                                                                                             | Builds the release app and installers                                   |
| `pnpm typecheck`                                                                                               | `tsc --noEmit` in every package                                         |
| `pnpm lint`                                                                                                    | Biome, stylelint, `turbo boundaries` and `cargo xtask check-boundaries` |
| `pnpm test`                                                                                                    | Frontend tests (vitest)                                                 |
| `cargo fmt --all --check` · `cargo clippy --workspace --all-targets -- -D warnings` · `cargo test --workspace` | Rust checks                                                             |

## Releasing

The Release workflow (`.github/workflows/release.yaml`) builds the installers:
Windows (NSIS `-setup.exe` and `.msi`), macOS (`.dmg`, Apple silicon and
Intel) and Linux (`.deb`, `.rpm`, `.AppImage`).

- **To try them:** run it by hand (Actions → Release → Run workflow). The
  installers are kept as the run's artifacts for 14 days, and nothing is
  published. A pull request that changes the workflow runs it too.
- **To release:** set the version in `apps/desktop/src-tauri/tauri.conf.json`,
  then push a tag `v` + that version (`v0.1.0`). The workflow checks they
  match, builds, and drafts a GitHub release with the installers and
  `SHA256SUMS.txt`. Only the repository's writers see a draft; publish it
  from the Releases page once it's tested.
- **On your own machine:** `pnpm release:auto` builds the installers for
  the system it runs on, the same kinds as the workflow (Windows: `.exe` and
  `.msi`; macOS: `.dmg`; Linux: `.deb`, `.rpm`, `.AppImage`), for this
  machine's processor. They're gathered with `SHA256SUMS.txt` in
  `target/installers/webide-<version>-<os>-<arch>/`. Nothing is uploaded.
  For one kind only: `pnpm tauri build --bundles nsis`.
- **Trying a build beside the one you use:** start it with
  `WEBIDE_CONFIG_DIR` set to an absolute folder of its own. It keeps its
  settings and recent folders there, so it opens none of yours.

The installers aren't signed yet, so Windows SmartScreen and macOS Gatekeeper
warn before they open one.

## Layout

```
crates/            Rust: ide-core, ide-fs, ide-watcher, ide-workspace, ide-search, ide-git,
                   ide-terminal, ide-lsp, ide-debug, ide-mcp, ide-assistant, ide-ipc
apps/desktop/      The Vite frontend (src/) and the Tauri composition root (src-tauri/)
packages/          Frontend modules: contracts, kernel, ipc, shell, explorer, tabs,
                   editor-surface, commands, settings, search, git, terminal, lsp, debug,
                   mcp, assistant, file-icons, and the shared tsconfig
xtask/             cargo xtask check-boundaries, check-csp, check-contracts
```

Each module has a README with its purpose, public API and allowed dependencies.

## Module rules (enforced by `pnpm lint`)

- Rust: only `ide-ipc` (and the composition root) may reach `tauri`. Feature
  crates depend on `ide-core` only. `ide-core` has no IO and no async runtime.
- Frontend: only `@repo/ipc` imports `@tauri-apps/*`. A feature package never
  imports another feature package or `@repo/ipc`; a shared building block
  (`@repo/file-icons`) depends on no other `@repo/*` package. Every package
  is imported through its public entry. CodeMirror only in
  `@repo/editor-surface`, xterm only in `@repo/terminal`.

## AI agents

File → AI Agents… turns on an MCP server, so AI agents such as Claude Code
can work in the open folder. It's off until turned on, listens on 127.0.0.1
only, needs the token shown there, and refuses requests from web pages.
Agents read files, search, and see git's changes; they write files only when
"Allow agents to edit files" is on, never in `.git`, and never run commands.
The dialog has the `claude mcp add …` command to copy, and lists what agents
did. Details: [crates/ide-mcp](crates/ide-mcp/README.md).

## AI assistant

View → Open the AI Assistant (Ctrl+Alt+I, Cmd+Alt+I on macOS) opens it in
the secondary side bar; File → Set Up the AI Assistant… is a step guide:
the provider (Anthropic, OpenAI, Azure AI Foundry, Gemini, Ollama or Amazon
Bedrock), how to reach it, the model, and a check that it answers. Keys go
to the system's credential store, and every request goes from the app's
own process. The assistant reads the folder with the same tools as agents,
and each change it proposes is shown as a diff that's written only when
you accept it. It runs no commands. Details:
[crates/ide-assistant](crates/ide-assistant/README.md),
[packages/assistant](packages/assistant/README.md).
