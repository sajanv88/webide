# ide-fs

**Status:** Milestone 3.

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

Git's own files are never written: a path with a `.git` segment (any case),
or one whose resolved target is inside a `.git` folder (through a symlink, a
new file in a linked folder included), fails with `PermissionDenied`. A hook
or a config line written there would run the next time git does. Reading
them works, read-only (`ReadOnlyReason::GitInternal`).

Known limits:

- A process that can already write inside the workspace could swap a folder
  for a symlink between the check and the open (TOCTOU). Closing that needs
  handle-relative IO, which std doesn't provide.
- A repository whose `.git` is a file pointing to a git folder with another
  name (`gitdir: ./repo.git`, some worktree setups) isn't covered by the
  `.git` rule.

## Public API

- `LocalFs` (implements `WorkspaceFs`).

## May depend on

`ide-core` only, among workspace crates (dev: `tempfile`). Never Tauri.
Checked by `cargo xtask check-boundaries`.
