# @repo/git

**Status:** Milestone 3.

## Purpose

Source control: the Source Control view in the side bar, the branch in the
status bar, and Switch Branch. Everything goes through the `GitPort` the app
binds to the open workspace.

- `createGitStore(port)` is the state, in plain TypeScript (tested without
  React): what git says, sorted into **merge conflicts**, **staged changes**
  and **changes** (a file changed in both the index and the working tree is
  in both). A refresh asked for while one runs is folded into one more, and
  each operation reads the state again when it's done.
- `SourceControlView`:
  - the branch ("On main", how many commits to push or pull, or "Detached at
    abc1234");
  - a commit message (Ctrl+Enter, or Cmd+Enter, commits) and a Commit button
    that says how many files it commits. An empty message, nothing staged, or
    git's refusal (a failing hook) is said on the field, with focus there;
  - each list is a nuvui `ListView`, named by its heading, with a button to
    stage or unstage them all. Each row: the file, its folder, its change as
    a letter (with the word said, and shown on hover), and buttons: Stage,
    Unstage, Discard changes (not for untracked files). Discarding asks
    first, in an alert dialog;
  - pressing a row (or Enter on it) shows its changes in a large dialog: the
    app's diff view (`renderDiff`), with Open file. Focus goes back to the
    row when it closes, or to the first file if that row is gone;
  - outside a repository, Initialize repository; without git, how to get it.
  - It reads git's state on mount, 300 ms after changes on disk settle (git's
    own files in `.git` included, so a commit made in a terminal shows up),
    and when the window comes back into focus.
- `BranchStatus`: the branch in the status bar, as a button that opens
  Switch Branch.
- `BranchPicker`: nuvui `CommandDialog` with the local branches (the current
  one can't be chosen), and "Create branch “…” and switch to it" for a valid
  new name typed.

Commands: `git.refresh`, `git.commit`, `git.stageAll`, `git.unstageAll`,
`git.switchBranch`, `git.init` (each while it applies).

### Known limits

- No push, pull, fetch, merge, stash or blame; no history view.
- Changes are staged by file, not by line.
- Diffs are shown read only.

## Public API

- `createGitStore(port)` → `GitStore`; `toView`; types `GitState`, `GitView`.
- `SourceControlView` with `{ store, readDiff, registry, bus, viewId,
  renderDiff, styleNonce? }`; `REFRESH_DELAY_MS`.
- `BranchStatus`, `BranchPicker` with `{ store, registry }`.
- `validBranchName`, `changeIn`; types `ListKind`, `DiffSlotProps`, `DiffRequest`.

## May depend on

`@repo/contracts` (`GitPort`, the DTOs, `AppEvents`), `@repo/kernel`,
`@repo/file-icons` (a file's icon by its type), `react`, `@nuvui/react`,
`@nuvui/icons`. Never another feature package,
`@repo/ipc` or `@tauri-apps/*`.
