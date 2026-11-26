# @repo/terminal

**Status:** Milestone 4.

## Purpose

Terminals in the shell's panel, drawn by xterm.js (nuvui has no terminal).
This is the only package that may import `@xterm/*`: Biome fails anywhere
else. Everything goes through the `TerminalPort` the app binds to the open
workspace.

- `TerminalPanel`: the terminals as nuvui `Tabs` ("Terminals"), named
  "1: pwsh", "2: zsh (exited)", with New terminal and Kill terminal buttons,
  or an empty state with New terminal. Every terminal stays mounted while
  another is shown, so its shell keeps running.
- `TerminalView`: one xterm. It fits the box it's in (from one character
  cell's size, as it changes), sends what's typed to the shell, and draws
  what comes back. When the shell exits, it says so in the terminal and the
  tab, and a screen reader is told.
- **Keys:** everything goes to the shell (Ctrl+C, Ctrl+W, Ctrl+B are the
  shell's), except Ctrl+C with a selection or Ctrl+Shift+C (copy), Ctrl+V or
  Ctrl+Shift+V (paste), and the keys the app's `passKey` says are its own.
- **Colors** come from the page's tokens (`--color-surface`, its text, the
  primary color for the selection), read again when the theme changes, with
  ANSI colors chosen for a dark or a light background.
- **A screen reader:** each terminal's input is named "Terminal 1: pwsh";
  `terminal.screenReader` turns on xterm's screen reader mode (the output is
  read as it comes).
- **Typed for the person:** `terminal.type` on the bus (`{ text }`) opens a
  new terminal and types `text` at its prompt once the shell's output has
  paused (400 ms), without pressing Enter: they read it and run it.
- Commands: `terminal.new`, `terminal.kill`, `terminal.focus` ("Toggle
  Terminal": show the panel, start a terminal if there's none, and focus it;
  from inside a terminal, hide the panel and go back), `terminal.screenReader`.

### The CSP

xterm has no `nonce` option, and the page's CSP has no `'unsafe-inline'`.
`allowTerminalStyles(nonce)` (installed the first time a terminal is drawn)
gives the page's nonce to each `<style>` made with `document.createElement`,
and turns `setAttribute("style", …)` into `style.cssText = …`, which the
policy allows (xterm draws 24-bit colors that way). Neither lets markup in:
a `<style>` or `style="…"` that arrives as HTML is still refused.

The app's Vite config also rewrites one assignment in xterm's bundled VS Code
code (`KeyCodeUtils.toString = …`), which throws under Tauri's frozen
`Object.prototype`; the build fails if that code changes.

### Known limits

- The font and its size aren't configurable yet; prompts that use Nerd Font
  symbols show boxes for them.
- No search in the terminal, no links, no split terminals.
- Closing the window doesn't ask about shells still running.

## Public API

- `TerminalPanel` with `{ port, registry, bus, passKey?, styleNonce? }`.
- `TerminalView` with its props; `fit(box, cell)`; type `TerminalStatus`.
- `terminalTheme`, `tokenColor`, `luminance`; `allowTerminalStyles`.

## May depend on

`@repo/contracts` (`TerminalPort`, the DTOs, `AppEvents`), `@repo/kernel`,
`react`, `@nuvui/react`, `@nuvui/icons`, `@xterm/xterm`. Never another
feature package, `@repo/ipc` or `@tauri-apps/*`.
