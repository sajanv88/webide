# ide-workspace

**Status:** Phase 2.

## Purpose

Implements `ide_core::WorkspaceStore`: opens a workspace folder (canonicalise,
check it's a folder, record it), keeps the recent list (most recent first,
deduplicated, at most 10), and persists settings. The data lives in two
versioned JSON files in the config folder the composition root passes in
(Tauri's app config dir):

- `recent-workspaces.json`
- `settings.json`: unknown keys are ignored and missing ones take defaults. A
  file that isn't valid settings is moved to `settings.invalid.json`, and the
  defaults are used.

Both are written atomically. The on-disk format is separate from the domain
types, so either can change.

## Public API

- `JsonWorkspaceStore::new(config_dir)` and `with_clock(config_dir, clock)` (for tests).

## May depend on

`ide-core` only, among workspace crates (plus `serde`, `serde_json`; dev:
`tempfile`). Never Tauri. Checked by `cargo xtask check-boundaries`.
