# @repo/shell

**Status:** Milestone 4.

## Purpose

The window frame, after nuvui's `app-shell` block:

- a top bar with the menubar, the title and tools;
- an activity bar (a vertical tab list named "Views") choosing which view
  fills a resizable, collapsible side pane, whose sizes are remembered in
  localStorage. Every view stays mounted, so each keeps its state while
  another is shown. Clicking the view that's shown hides or shows the pane;
  the arrow keys only move between views;
- the work area, and under it a resizable, collapsible panel (the terminals),
  hidden until shown, its size remembered; and after it a secondary side
  bar (the assistant), the same way, with a button in the top bar;
- a status bar;
- with a `frame`, the window's own frame, the same on every system: the top
  bar is the title bar (the title in the middle, its empty parts move the
  window, a double click maximizes or restores it), with minimize,
  maximize/restore and close at the end on Windows and Linux (on macOS the
  system's buttons stay, at the start). A 1px line around the window is the
  accent colour while it has focus and the border colour without, and goes
  away while it's maximized or full screen. The title fades without focus.

It fills named slots and doesn't know what's in them.

The menubar is built from the command registry. Each item names a command id,
and its title, its enabled state and what it does come from the registry. A
command nobody has registered (Save while no folder is open, say) is left out,
with the separators that would then lead, trail or double up; a menu left with
nothing in it stays in its place, disabled. Each item shows the keys bound
to its command (from `shortcutFor`), with `aria-keyshortcuts`.
While there are views, the shell registers `view.toggleSidebar` (the top bar
shows a button for it, with `aria-expanded`), `view.showView` (args: a view's
`id`; not in the palette), and each view's own `command`, which shows it.
With a panel, it registers `view.togglePanel` and `view.showPanel` (not in the
palette). With an aside, `view.toggleAside` and `view.showAside`. Hiding
either while focus is in it gives focus back to where it was before.

## Public API

- `Shell` with `ShellProps`:
  - `title`, `registry`, `menus`;
  - `shortcutFor?(command)` → `{ label, aria }`: the keys shown in the menus;
  - `views?`: `SidebarView`s (`id`, `title`, `icon`, `content`, `command?`),
    the first shown first; without any, the work area fills the window;
  - `main`, `panel?` (`{ title, content }`: a region named `title`),
    `aside?` (`{ title, icon, content }`: a complementary landmark named
    `title`), `statusBar?`;
  - `frame?`: `WindowFrame` (`os`, `maximized`, `fullscreen`, and
    `minimize`, `toggleMaximize`, `close`, `startDrag`).
- `WindowFrame`.
- `SidebarView`.
- `MenuSpec`, `MenuShortcut`, `MenuItemSpec`: `command` (with an optional `label` and `args`),
  `separator`, and `submenu` (shown disabled when empty).

Landmarks: `banner` "Title bar", `menubar` "Application", `main` "Workspace",
`contentinfo` "Status bar". The pane handle is a `separator` named "Resize side bar".

## May depend on

`@repo/contracts`, `@repo/kernel`, `react`, `@nuvui/react`, `@nuvui/icons`.
Never another feature package, `@repo/ipc` or `@tauri-apps/*`.
