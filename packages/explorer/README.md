# @repo/explorer

**Status:** Phase 5.

## Purpose

The open folder as a tree, on nuvui `Tree`, styled after nuvui's
`file-browser` block. Folders load lazily: a folder is listed through the port
only when it's opened, one level at a time.

- A folder that's loading, empty or failed shows a disabled row saying so.
  A failed folder is fetched again when it's reopened.
- A folder with more than `pageSize` entries (500 by default) shows the first
  500 and a "Show 500 more" row, because nuvui's Tree doesn't virtualise. The
  measurements are below.
- Choosing a file emits `explorer.fileActivated` on the bus.
- Changes on disk (`workspace.filesChanged` on the bus) list again only the
  folders already listed that contain a change; a removed folder is forgotten,
  with what was open under it. A rescan reloads every open folder.
- While mounted, it registers `explorer.refresh` and `explorer.collapseAll`.
  They're also on its toolbar buttons and in its context menu.

The tree resets when the workspace changes (it's keyed by the workspace id).

### Tree limits (measured in the release build, WebView2, 2026-11-20)

| Rows drawn in one open folder | Re-render after "Show more"                                |
| ----------------------------- | ---------------------------------------------------------- |
| 1,000                         | 43 ms                                                      |
| 3,000                         | 60 ms                                                      |
| 5,000                         | 70 ms                                                      |
| 7,000                         | 115 ms                                                     |
| 9,000                         | 128 ms                                                     |
| 10,000                        | 287 ms (reopening a folder with 10,000 rows drawn: 506 ms) |

Opening any folder at the 500-row page took 44–57 ms whatever its size. Arrow
keys respond in about 33 ms at 500 and at 2,000 rows, and 83 ms at 10,000. The
backend lists at most 10,000 entries per folder.

## Public API

- `Explorer` with `ExplorerProps`: `source: { workspace, port }`, `registry`,
  `bus`, `styleNonce?`, `pageSize?`.
- `ExplorerSource`, `DEFAULT_PAGE_SIZE`.

## May depend on

`@repo/contracts` (`ExplorerPort`, the DTOs, `AppEvents`, `AppCommandId`),
`@repo/kernel`, `react`, `@nuvui/react`, `@nuvui/icons`. Never another
feature package, `@repo/ipc` or `@tauri-apps/*`.
