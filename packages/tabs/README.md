# @repo/tabs

**Status:** Phase 6.

## Purpose

The open documents of one workspace, and their tabs: which files are open, in
what order, which is active, which have unsaved changes, saving them, and the
question asked before closing one that has.

- `createDocuments(port)` is the state, in plain TypeScript (tested without
  React). It reads and writes files only through the `DocumentsPort` the app
  gives it.
- `DocumentTabs` draws it on nuvui `Tabs`, over one panel. The panel shows the
  editor the app renders into the `renderEditor` slot. This package never
  imports the editor: the app wires `@repo/editor-surface` in.

### What it does

- A file chosen in the explorer (`explorer.fileActivated`) opens in a new tab
  next to the active one, or activates the tab it's already in. Two open files
  with the same name are labelled with their folders.
- Text is kept as the editor holds it, with `\n` line breaks. Saving puts the
  file's own line endings back (CRLF stays CRLF). A file with mixed endings
  says so, and is saved with LF.
- **Unsaved changes:** a dot in place of the tab's ×, and ", unsaved changes"
  in the tab's name, so it doesn't rely on colour. Typing back to what's on
  disk makes the tab clean again.
- **Closing** asks "Save changes to …?" (Save / Don't save / Cancel) when a tab
  has unsaved changes, once for all of them when several close together. A
  file whose save fails stays open.
- **Files that aren't opened as text** open as a notice: too large (over the
  "refuse above" setting), binary, or not UTF-8. A file over the "read-only
  above" setting, or read-only on disk, opens read-only with a bar saying why.
- Reloading the page with unsaved changes asks first (`beforeunload`).
- **Changes on disk** (`workspace.filesChanged` on the bus): each open file
  under a changed path is read again and compared with what was last read or
  saved. The same text (its own save, a touch) changes nothing. A clean
  document takes the new text: the editor gets it through its handle
  (`setValue`, so it can be undone), now or when its tab is next shown. One
  with unsaved changes shows "changed on disk" with Reload from disk / Keep my
  changes. A file that's gone shows "deleted", keeps its text, and Save
  creates it again. A check that arrives mid-save waits for the save.

### Keyboard and mouse

| To                  | Do                                                                         |
| ------------------- | -------------------------------------------------------------------------- |
| Move between tabs   | ← → on the tab row (Home, End)                                             |
| Close a tab         | Delete on the tab, its context menu, File → Close Editor, or the × (mouse) |
| Close others or all | The tab's context menu, or the File menu                                   |
| Reorder             | Drag a tab, or Move Left / Move Right in its context menu                  |
| Save                | Ctrl+S (Cmd+S), Ctrl+Alt+S for all, or File → Save / Save All              |
| Next / previous tab | Ctrl+PageDown / Ctrl+PageUp, from anywhere                                 |

The keys outside the tab row are the app's keybindings (`@repo/commands`).

The × beside each tab is for the mouse only (`aria-hidden`, out of the tab
order): a button inside a tab list isn't valid ARIA. Every way to close it
works from the keyboard.

### Commands it registers while shown

`file.save`, `file.saveAll`, `tabs.close`, `tabs.closeOthers`, `tabs.closeAll`,
`tabs.next`, `tabs.previous`.

## Public API

- `createDocuments(port, { onError? })` → `Documents`: `getState`, `subscribe`,
  `open`, `activate`, `reorder`, `getText`, `setText`, `save`, `saveAll`,
  `requestClose` (resolves to whether everything closed), `answerClose`,
  `hasUnsaved`, `checkDisk(paths | null)`, `reload`, `keepMine`.
- Types: `DocumentId` (branded), `DocumentView` (with `disk` and `revision`),
  `DiskState`, `DocumentContent`,
  `DocumentsState`, `PendingClose`, `CloseChoice`, `DocumentsOptions`.
- `DocumentTabs` with `DocumentTabsProps`: `documents`, `registry`, `bus`,
  `renderEditor(props: EditorSlotProps)`, `empty`. Pass `props.ref` to the
  editor (an `EditorSlotHandle`: `setValue`), so reloads reach it.
- `toEditorText`, `toDiskText`, `endingAfterSave`: the line-ending rules.

## May depend on

`@repo/contracts` (`DocumentsPort`, the DTOs, `AppEvents`, `AppCommandId`),
`@repo/kernel`, `react`, `@nuvui/react`, `@nuvui/icons`. Never another feature
package (the editor included), `@repo/ipc` or `@tauri-apps/*`.
