# @repo/kernel

**Status:** Phase 6: the event bus and the command registry, which menus,
keybindings and the palette (in `@repo/commands`) are all built from.

## Purpose

How modules talk without importing each other. A feature registers commands
and listens for events. Menus, keybindings and the palette are built from the
registry, so no list of commands is written twice.

A command is `id`, `title`, `category?` (the palette's group, "File: Save"),
`isEnabled?` (when it can run: VS Code's `when`; menus grey it out, the
palette leaves it out, its keys do nothing), `palette?` (`false` for one that
needs arguments) and `run(args)`.

## Public API

- `createCommandRegistry<Id>({ onError? })` → `CommandRegistry<Id>`:
  - `register(command)` returns a dispose function. A duplicate id throws.
  - `get`, `all`, `isEnabled`.
  - `execute(id, args?)` never throws. It resolves to
    `{ ok: true } | { ok: false, reason: "notRegistered" | "disabled" | "failed" }`.
  - `subscribe`, `notify` and `version`, for `useSyncExternalStore`.
- `createEventBus<Events>({ onListenerError? })` → `EventBus<Events>`: `on(type, listener)`,
  which returns the unsubscribe, and `emit(type, payload)`. One listener that
  throws doesn't stop the others.
- Types: `Command`, `ExecuteResult`, `Dispose`, `Listener`.

The app's command ids and events are typed in `@repo/contracts`
(`AppCommandId`, `AppEvents`).

## May depend on

Nothing: plain TypeScript. No React, no nuvui, no IPC.
