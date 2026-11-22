# @repo/debug

**Status:** Milestone 6.

## Purpose

Run and Debug: runs the JavaScript or TypeScript file shown under Node's
debugger, through the `DebugPort` the app binds to the open workspace.

- `createDebugStore(port, bus)`: the debugger's state for one workspace:
  `idle`, `starting`, `running`, `paused` or `ended`; the breakpoints by file
  (kept when nothing runs, sent when something does); the call stack, the
  paused frame and its scopes; the output (the last 2,000 lines). It hears
  `debug.event` on the bus. When the program stops, it opens the top frame
  that's in the workspace at its line (`file.openRequested`) and reads its
  cheap scopes' variables (the globals wait until opened).
- `DebugView`: the side bar's Run and Debug view. A button runs the file
  shown; while it runs, a toolbar (Continue or Pause, Step over, into, out,
  Stop), what it's doing (a `status` region), the call stack (a frame chosen
  shows its line and variables), the variables as a tree (an object opens to
  its properties, read then), the breakpoints (each with a remove button), and
  the debug console (`role="log"`) with a box to evaluate in the paused frame.
- The editor draws the breakpoints and the paused line: the app passes
  `breakpoints`, `onToggleBreakpoint` and `executionLine(path)` to
  `EditorSurface` for files `debuggable(path)` says can run.
- Commands: `debug.start` (F5; while paused it continues), `debug.stop`
  (Shift+F5), `debug.continue`, `debug.pause`, `debug.stepOver` (F10),
  `debug.stepInto` (F11), `debug.stepOut` (Shift+F11). `view.showDebug`
  (Ctrl+Shift+D) is the shell's.

### Known limits

- Breakpoints aren't kept when the workspace closes, and don't move when lines
  are added above them until the program runs again.
- One thread: the program's main one.

## Public API

- `createDebugStore`, types `DebugStore`, `DebugState`, `Phase`, `OutputLine`,
  `PausedScope`; `MAX_OUTPUT`; `debuggable(path)`.
- `DebugView` with `{ store, registry, currentFile, viewId, styleNonce? }`.

## May depend on

`@repo/contracts` (`DebugPort`, the DTOs, `AppEvents`), `@repo/kernel`,
`react`, `@nuvui/react`, `@nuvui/icons`. Never another feature package,
`@repo/ipc` or `@tauri-apps/*`.
