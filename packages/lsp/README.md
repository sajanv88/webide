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
  - `restart()` stops the servers and opens the files again in new ones.
  - It sends the files a server handles: scripts, JSON, CSS, SCSS, Less,
    HTML and YAML. The backend picks the server.
- `LanguageStatusItem`, for the status bar: each server by name (TypeScript,
  JSON, YAML…) as it starts (a spinner) and is ready, with the open files'
  errors and warnings (all said in words). One that failed is a button that
  restarts them; one that needs Node.js is a "Node.js needed" button; one
  that's off until the folder is trusted (`restricted`) is a button to
  `workspace.manageTrust`. It registers `language.restart` ("Restart the
  Language Servers").
- `NodeSetupDialog`: asks for Node.js the first time a server needs it (and
  from that button). On Windows and macOS, "Open the terminal" types the
  system's installer command into a new terminal (`terminal.type`) for the
  person to read and run: `winget install --id OpenJS.NodeJS.LTS -e`, or
  `brew install node`. Or they install it themselves, from
  nodejs.org/en/download (shown, not opened; on Linux, the only choice).
  Check again restarts the servers, which look for it again.

## Public API

- `createLanguageClient(port, bus)` → `LanguageClient` (`handles`, `features`,
  `problems`, `restart`, `dispose`); `placeProblems(text, problems)`;
  `CHANGE_DELAY_MS`, `PROBLEMS_DELAY_MS`; types `LanguageFeatures`, `Position`.
- `LanguageStatusItem` with `{ client, registry, bus, platform? }`.
- `NodeSetupDialog` with `{ open, onOpenChange, servers, bus, onCheckAgain,
  platform? }`; `currentPlatform`, `INSTALL_COMMAND`, `DOWNLOAD_ADDRESS`,
  `listed`, type `Platform`.

## May depend on

`@repo/contracts` (`LanguagePort`, the DTOs, `AppEvents`), `@repo/kernel`,
`react`, `@nuvui/react`, `@nuvui/icons`. Never another feature package,
`@repo/ipc` or `@tauri-apps/*`.
