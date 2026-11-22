# ide-fs

**Status:** Phase 2.

## Purpose

Implements `ide_core::WorkspaceFs` on the local disk: list one level of a
folder, read a file, write a file atomically.

Every operation runs the same guard before any IO on the target:

1. The path is a `RelPath`, already validated in `ide-core`.
2. The root is canonicalised again, and so is the target, following every
   symlink and junction. The target must be inside the canonical root,
   compared by whole components, or the call fails with `OutsideWorkspace`.
   A new file is checked through its (existing, canonicalised) parent folder.

Writes go to a temporary file beside the target (`TEMP_FILE_PREFIX`), are
flushed, keep the original's permissions, and are renamed over it. Read-only
files are refused, and writes never follow a dangling symlink.

Known limit: a process that can already write inside the workspace could swap
a folder for a symlink between the check and the open (TOCTOU). Closing that
needs handle-relative IO, which std doesn't provide.

## Public API

- `LocalFs` (implements `WorkspaceFs`).

## May depend on

`ide-core` only, among workspace crates (dev: `tempfile`). Never Tauri.
Checked by `cargo xtask check-boundaries`.
