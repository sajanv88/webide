# ide-watcher

**Status:** Milestone 2.

## Purpose

Watches a workspace for changes made by anyone (this app, other programs, git)
and reports them as `ide_core::watch` events.

- One recursive OS watch per workspace: ReadDirectoryChangesW on Windows,
  FSEvents on macOS, inotify on Linux (through `notify` 8.2 and
  `notify-debouncer-full` 0.7). Symlinks aren't followed.
- Debounced (200 ms), so a burst of writes arrives as one batch, with one
  change per path (`coalesce`). More than 1,000 changes in a batch, or a
  rescan notice from the system, arrive as `rescan: true`.
- Paths become workspace-relative `RelPath`s. Anything outside the root, with
  a name a `RelPath` can't hold, or named like our atomic-write temporary
  files (`.~ide-tmp-*`) is left out, so a save reports only the file it
  replaced.
- The watch starts on a thread of its own, so opening a workspace doesn't
  wait for it. A watch that can't start, or fails later, reports `Failed`.
- No file-id cache: the debouncer's default one walks the whole tree when the
  watch starts. Without it, a move the system reports as two separate names
  arrives as `Removed` and `Created`, which is all the frontend needs.
- Each batch is checked against the disk before it's sent: a path that's
  gone is `Removed`, and a "removed" path that's there again is `Modified`.
  Systems don't always say what happened (FSEvents merges a file's flags, so
  a file created and deleted moments apart can arrive as `Modified`). The
  root's own creation (FSEvents can report it late) is left out.

### Known limits

- On Linux, inotify watches a new folder only once it has seen it created,
  so files written into a brand-new folder at once (an unzip, a checkout)
  can arrive as the folder alone. That's enough for the explorer, which lists
  a new folder when it's opened.
- On Linux, inotify needs one watch per folder. A workspace with more folders
  than `fs.inotify.max_user_watches` (often 65,536 or more; 8,192 on older
  systems) fails to watch past that, and says so (`Failed`). Nothing is
  excluded yet (`node_modules` included).
- Changes inside a folder reached through a symlink aren't reported.

## Public API

- `LocalWatcher` (`new`, `with_debounce`), implementing `ide_core::FsWatcher`.
- `DEFAULT_DEBOUNCE`.

## May depend on

`ide-core` only, among workspace crates (plus `notify-debouncer-full`, which
brings `notify`). Never Tauri. Checked by `cargo xtask check-boundaries`.
