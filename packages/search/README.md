# @repo/search

**Status:** Milestone 2.

## Purpose

Find in files (the Search view in the side bar) and Go to File (quick open).
Both search through the `SearchPort` the app binds to the open workspace.

### Search view

- A search form (`role="search"`, "Find in files"): the text, three toggles
  (Match case, Match whole word, Use regular expression, each with
  `aria-pressed`), and file globs to include or exclude behind a toggle
  ("src/**, *.ts", gitignore syntax).
- It searches once typing pauses for 250 ms; Enter, or the refresh button,
  searches at once. A newer search replaces an older one, whose answer is
  dropped.
- What it found is a nuvui `Tree` ("Search results"): each file with how many
  lines matched, and each line with its number and every match in a `<mark>`
  (bold and tinted, not told by color alone). 1,000 lines are drawn, then a
  "Show more" row.
- Choosing a line emits `file.openRequested` with its path, line and column:
  the tabs open it there and the editor takes focus.
- A status line (`role="status"`) says "3 results in 2 files.", "No results
  for …", or that the search stopped at its limit. An invalid regular
  expression or glob is said on the field, with `aria-invalid`.
- It registers `search.findInFiles`, which shows the view
  (`view.showView`, with the `viewId` it's given) and puts focus in the field.

### Go to File

- nuvui `CommandDialog` ("Go to file"), from `search.goToFile`.
- It lists the workspace's files (ignored ones left out), read when it first
  opens and again after files are added, removed or renamed.
- What's typed matches loosely: its characters in order, ignoring case and
  spaces ("dtabs" finds `document-tabs.tsx`). Matches in the file's name rank
  first, then runs of consecutive characters and the starts of words. The
  best 50 are shown, the name first with what matched marked, then the
  folder.
- Choosing one emits `file.openRequested` with its path.

### Known limits

- Results aren't updated when files change; search again to see changes.
- No replace yet.

## Public API

- `SearchView` with `{ port, registry, bus, viewId, styleNonce? }`.
- `QuickOpen` with `{ port, registry, bus }`.
- `matchFile(path, query)`, `matchFiles(files, query, limit)`, `splitGlobs(text)`,
  and `SEARCH_DELAY_MS`, `RESULT_PAGE`, `QUICK_OPEN_LIMIT`.

## May depend on

`@repo/contracts` (`SearchPort`, the DTOs, `AppEvents`), `@repo/kernel`,
`react`, `@nuvui/react`, `@nuvui/icons`. Never another feature package,
`@repo/ipc` or `@tauri-apps/*`.
