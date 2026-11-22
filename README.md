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
| Ctrl+B                      | Show or hide the side bar |
| Ctrl+,                      | Settings                  |

On macOS, Ctrl+PageDown, Ctrl+PageUp and Ctrl+Shift+G stay on the Control key. They're one
table, [apps/desktop/src/keybindings.ts](apps/desktop/src/keybindings.ts);
the menus and the palette show them from there.

## Requirements

- Node 24+, pnpm (the version in `packageManager` is fetched by pnpm itself)
- Rust via rustup (the toolchain in `rust-toolchain.toml` is installed on first use)
- The [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS

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

## Layout

```
crates/            Rust: ide-core, ide-fs, ide-watcher, ide-workspace, ide-search, ide-git, ide-ipc
apps/desktop/      The Vite frontend (src/) and the Tauri composition root (src-tauri/)
packages/          Frontend modules: contracts, kernel, ipc, shell, explorer, tabs,
                   editor-surface, commands, settings, search, git, and the shared tsconfig
xtask/             cargo xtask check-boundaries, check-csp, check-contracts
```

Each module has a README with its purpose, public API and allowed dependencies.

## Module rules (enforced by `pnpm lint`)

- Rust: only `ide-ipc` (and the composition root) may reach `tauri`. Feature
  crates depend on `ide-core` only. `ide-core` has no IO and no async runtime.
- Frontend: only `@repo/ipc` imports `@tauri-apps/*`. A feature package never
  imports another feature package or `@repo/ipc`. Every package is imported
  through its public entry. No code editor library anywhere.

## Reserved names

Not built in this milestone: `ide-lsp`, `ide-terminal`, `ide-git`, `ide-debug`,
`ide-mcp`, `ide-search`, and `@repo/lsp`, `@repo/terminal`, `@repo/git`,
`@repo/debug`, `@repo/mcp`, `@repo/search`. The boundary rules already treat
them as feature modules.
