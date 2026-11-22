# @repo/shell

**Status:** Phase 6.

## Purpose

The window frame, after nuvui's `app-shell` block:

- a top bar with the menubar, the title and tools;
- a resizable, collapsible explorer pane, whose sizes are remembered in localStorage;
- the work area;
- a status bar.

It fills named slots and doesn't know what's in them.

The menubar is built from the command registry. Each item names a command id,
and its title, its enabled state and what it does come from the registry. A
command nobody has registered (Save while no folder is open, say) is left out,
with the separators that would then lead, trail or double up; a menu left with
nothing in it stays in its place, disabled. Each item shows the keys bound
to its command (from `shortcutFor`), with `aria-keyshortcuts`.
While there's a sidebar, the shell registers `view.toggleExplorer`, and the top
bar shows a button for it with `aria-expanded`.

## Public API

- `Shell` with `ShellProps`:
  - `title`, `registry`, `menus`;
  - `shortcutFor?(command)` → `{ label, aria }`: the keys shown in the menus;
  - `sidebar?`: without it, the work area fills the window;
  - `main`, `statusBar?`.
- `MenuSpec`, `MenuShortcut`, `MenuItemSpec`: `command` (with an optional `label` and `args`),
  `separator`, and `submenu` (shown disabled when empty).

Landmarks: `banner` "Title bar", `menubar` "Application", `main` "Workspace",
`contentinfo` "Status bar". The pane handle is a `separator` named "Resize explorer".

## May depend on

`@repo/contracts`, `@repo/kernel`, `react`, `@nuvui/react`, `@nuvui/icons`.
Never another feature package, `@repo/ipc` or `@tauri-apps/*`.
