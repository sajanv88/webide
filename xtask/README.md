# xtask

Workspace tasks, run with `cargo xtask <task>` (alias in `.cargo/config.toml`).

## Tasks

- `check-boundaries`: reads `cargo metadata` and fails when:
  - a crate other than `ide-ipc` or `ide-desktop` reaches `tauri` (directly or transitively);
  - `ide-core` depends on a workspace crate or reaches an async runtime;
  - a feature crate (`ide-fs`, `ide-watcher`, `ide-workspace`, and the reserved
    `ide-lsp`, `ide-terminal`, `ide-git`, `ide-debug`, `ide-mcp`, `ide-search`)
    or `ide-ipc` depends on a workspace crate other than `ide-core`;
  - an `ide-*` crate isn't classified in `src/main.rs`.

- `check-csp`: reads `apps/desktop/src-tauri/tauri.conf.json` and fails when
  the production CSP is missing, allows any `'unsafe-*'` source or a bare `*`,
  lacks `default-src`, `script-src`, `style-src` or `object-src`, or sets
  `dangerousDisableAssetCspModification`.

- `check-contracts`: runs the contracts export (`crates/ide-ipc/src/export.rs`)
  into a temporary folder and fails when any generated file in
  `packages/contracts/src/generated`, `apps/desktop/src-tauri/ipc_commands.rs` or
  `apps/desktop/src-tauri/permissions/ipc-commands.toml` differs, is missing,
  or is no longer generated. Fix with `pnpm contracts:generate`.

`pnpm lint` runs all three.

## May depend on

`serde_json` only. It's tooling: nothing depends on it.
