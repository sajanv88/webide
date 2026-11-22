# @repo/settings

**Status:** Phase 6.

## Purpose

The settings screen: File → Settings…, `settings.open` in the palette, or
Ctrl+, (Cmd+, on macOS). A nuvui `Dialog` with a form:

- **Theme:** Same as the system, Light or Dark (a radio group).
- **Open read-only above** and **Don't open above**, in megabytes (number
  fields, 0.1 to 256). The second must be at least the first; the form says
  so on the field and moves focus there. Both apply to files opened from then
  on.

It reads the settings each time it opens and saves them through the
`SettingsPort` the app gives it. The backend checks them again and keeps
them in the app's config folder. A failed save keeps the dialog open with the
reason. After a save it emits `settings.changed` on the bus; the app applies
the theme from it.

## Public API

- `SettingsDialog` with `{ port: SettingsPort, registry, bus }`. While mounted,
  it registers `settings.open`.

## May depend on

`@repo/contracts` (`SettingsPort`, `SettingsDto`, `AppEvents`),
`@repo/kernel`, `react`, `@nuvui/react`. Never another feature package,
`@repo/ipc` or `@tauri-apps/*`.
