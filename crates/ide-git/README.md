# ide-git

**Status:** Milestone 3.

## Purpose

Git for the open workspace, by running the person's own `git` program. It's
complete, it follows their config and credentials, and it runs the
repository's hooks, as git on the command line does.

Every run:

- goes straight to the `git` program, never through a shell, with arguments
  as a list (nothing is quoted or split);
- runs in the workspace folder, with `--literal-pathspecs`, so a path is
  never a glob or pathspec magic (`[ab].txt` is that file, not `a.txt`);
- has no terminal prompts (`GIT_TERMINAL_PROMPT=0`, `GIT_EDITOR=true`), no
  pager, no color, and `core.fsmonitor` off (a configured fsmonitor is a
  program git runs on each status);
- reads stdout and stderr as it runs, and is stopped past its time limit
  (30 s to read, 60 s to change, 5 minutes for a commit, which runs hooks);
- on Windows, opens no console window.

What it does:

- **State:** `git rev-parse --show-prefix`, then
  `git status --porcelain=v2 --branch -z --untracked-files=all -- .`. The
  workspace can be the repository's top or a folder in it: paths come back
  workspace-relative, and files outside it are left out. No git:
  `Unavailable`; not in a repository: `NotARepository`.
- **Diff:** the two versions of a file. Staged: the last commit and the index.
  Unstaged: the index and the working tree. Each version's size is checked
  before it's read (`git cat-file -s`, the file's metadata); over the limit
  is `TooLarge`, a NUL byte or not UTF-8 is `Binary`.
- **Stage** (`git add -A`, so deletions are staged), **unstage**
  (`git restore --staged`, or `git rm --cached` before the first commit),
  **discard** (`git restore`: the working tree back to the index; untracked
  files aren't touched).
- **Commit:** the message on stdin (`--file=-`), as written; the repository's
  hooks run. Git's refusal (nothing staged, a failing hook) comes back in its
  own words.
- **Branches:** the local ones (`for-each-ref`), switching to one, or making
  one from the current commit (`git switch --create`), with the name checked
  first.
- **Init:** `git init` in the workspace folder.

### Known limits

- No push, pull, fetch, merge, rebase, stash or blame yet.
- Hooks run on commit: that's what the repository asks for, and it's the
  person who commits. The editor can't write hooks (`ide-fs` refuses writes
  in `.git`), but a hooks folder set with `core.hooksPath` outside `.git`
  (husky's `.husky`) holds ordinary workspace files, as it does in any editor.

## Public API

- `LocalGit`, which implements `ide_core::GitClient`.

## May depend on

`ide-core` only (it runs `git` with `std::process`). Never another workspace
crate, Tauri or an async runtime. Checked by `cargo xtask check-boundaries`.
