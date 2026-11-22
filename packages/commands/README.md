# @repo/commands

**Status:** Phase 6.

## Purpose

Keybindings and the command palette, on the registry in `@repo/kernel`.
Neither keeps its own list of commands: the palette lists what's registered,
and keys name commands by id.

### Keybindings

The app writes one table (`apps/desktop/src/keybindings.ts`):
`{ key: "Mod+Shift+P", command: "commands.showPalette" }`.

- `Mod` is Ctrl, or Command on macOS. `Ctrl`, `Alt`, `Shift`, `Meta` are
  those keys everywhere.
- Letters and symbols are matched by their place on the keyboard
  (`KeyboardEvent.code`), so Shift and the keyboard layout don't change which
  key it is.
- Labels follow the platform: `Ctrl+Shift+P` on Windows and Linux, `⇧⌘P` on
  macOS. `aria` is the same keys for `aria-keyshortcuts` (`Control+Shift+P`,
  `Meta+Shift+P`).
- Problems are reported, not thrown: a key two commands claim stays with the
  first in the table, and an unreadable key is left out. Both are in
  `problems` (the app logs them as warnings).
- `attachKeybindings(window, …)` runs a bound command when it's enabled. It
  leaves a key alone when something already handled it (`defaultPrevented`:
  the editor's own keys), when it's pressed inside a dialog, or when its
  command is disabled, so the key keeps whatever it does by default.

### The palette

nuvui `CommandDialog`, opened by `commands.showPalette` (which it registers).
It lists the commands that can run now (`isEnabled`), leaves out those with
`palette: false` (they need arguments), groups them by `category` in
alphabetical order, shows each one's keys (`aria-keyshortcuts` too), and
closes before running the chosen one. Typing matches loosely ("file sav"
finds File: Save), and a command's id is also a keyword.

## Public API

- `createKeybindings(specs, platform)` → `Keybindings`: `all`, `forCommand(id)`
  (`label`, `aria`), `match(keyPress)`, `problems`.
- `attachKeybindings(target, keybindings, registry)` → `Dispose`.
- `detectPlatform()` → `"mac" | "other"`.
- `CommandPalette` with `{ registry, keybindings }`; `paletteGroups`.
- Types: `KeybindingSpec`, `Keybinding`, `Keybindings`, `KeybindingProblem`,
  `KeyPress`, `Platform`.

### Not yet

- Keys can't be changed by the person yet: the table is the app's. Chords
  (`Ctrl+K Ctrl+S`) aren't supported.

## May depend on

`@repo/contracts`, `@repo/kernel`, `react`, `@nuvui/react`. Never another
feature package, `@repo/ipc` or `@tauri-apps/*`.
