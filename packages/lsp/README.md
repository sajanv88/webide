# @repo/lsp

**Status:** Milestone 5.

## Purpose

The editor's side of the language server, through the `LanguagePort` the app
binds to the open workspace.

- `createLanguageClient(port, bus)`:
  - follows the `document.opened`, `document.changed` and `document.closed`
    events (from `@repo/tabs`) for script files, and keeps the server in step:
    a change is sent once typing pauses for 150 ms, and before any question
    about that file;
  - asks for problems when a file opens and 500 ms after changes settle (for
    every open file, up to 20: a change in one can change another's), places
    them by character offset in the file's text, and sends them as
    `language.problems`;
  - answers the editor (`features(path)`, the shape of the editor surface's
    `SurfaceLanguage`): completions, hover (Markdown) and go to definition,
    which opens a workspace file at the place (`file.openRequested`) or says
    where it is when it's outside the folder;
  - completions from modules not imported yet, with where they come from
    and, once picked, the import they add (`resolveCompletion`); the fixes
    for the problems at a place (`fixes`); and the edits that organize,
    sort or remove unused imports, or fix all (`sourceAction`). Each sends
    the file's latest text first.
  - `restart()` stops the server and opens the files again in a new one.
- `LanguageStatusItem`, for the status bar: TypeScript starting (a spinner),
  ready (with the open files' errors and warnings, said in words), or failed
  (pressing it restarts). It registers `language.restart`.

## Public API

- `createLanguageClient(port, bus)` → `LanguageClient` (`handles`, `features`,
  `problems`, `restart`, `dispose`); `placeProblems(text, problems)`;
  `CHANGE_DELAY_MS`, `PROBLEMS_DELAY_MS`; types `LanguageFeatures`, `Position`.
- `LanguageStatusItem` with `{ client, registry, bus }`.

## May depend on

`@repo/contracts` (`LanguagePort`, the DTOs, `AppEvents`), `@repo/kernel`,
`react`, `@nuvui/react`, `@nuvui/icons`. Never another feature package,
`@repo/ipc` or `@tauri-apps/*`.
