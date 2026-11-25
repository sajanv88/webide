# @repo/editor-surface

**Status:** Milestone 6.

## Purpose

The `EditorSurface` contract, and its implementation on nuvui's `CodeEditor`
(`@nuvui/editor/code`, which runs CodeMirror); and `DiffSurface`, two
versions of a file on nuvui's `DiffEditor`. This is the only package that
may import `@nuvui/editor` or an editor engine: Biome fails anywhere else. To
change the editor, change this package only.

- One editor shows every open document. Its `path` is `<documentId>/<file
  path>`, so each open document keeps its own undo history, selection and
  scroll, a reopened file starts fresh, and the language comes from the file's
  extension (fetched the first time it's used, from the app's own bundle).
- Uncontrolled: `initialValue` is read the first time a document is shown,
  never written back on each keystroke. `onChange` reports the whole text.
- Ctrl+S, or Cmd+S, anywhere in the editor (its search panel included) calls
  `onSave`.
- Under the code, the status line: where the cursor is, and the language.
- The CSP nonce goes to CodeMirror, which adds its styles as it runs.
- Loaded the first time it's drawn (`React.lazy`, with a spinner): CodeMirror
  is most of its weight, and the welcome screen doesn't need it.
- `focus()` and `revealPosition()` wait (up to a second) for a document just
  shown to be drawn, so a file opened at a line gets its cursor there.
- **Problems** (`diagnostics`, by character offset) are underlined and marked
  in the gutter.
- **Language features** (`language`, a `SurfaceLanguage`): completion as you
  type (after a word starts or a dot; Ctrl+Space asks), added to what the
  language suggests itself; hover, the server's Markdown drawn as text (code
  as code), never as HTML; go to definition with F12 or Ctrl+click (Cmd on
  macOS), which then doesn't add a cursor. These are CodeMirror extensions
  (`@codemirror/state`, `view`, `autocomplete`, the versions nuvui uses), made
  once; they ask the `language` of the document shown.
- **Breakpoints** (`breakpoints`, `onToggleBreakpoint`): a gutter beside the
  line numbers, where a press (or F9, on the cursor's line) toggles a line's
  breakpoint, drawn as a dot. Without `onToggleBreakpoint` the gutter does
  nothing. **The paused line** (`executionLine`) is tinted, and outlined in
  forced colors.
- **How lines are drawn** (`display`, from the settings; defaults until the
  app passes it):
  - **Indent guides:** a thin line down each level of indentation. A blank
    line takes the smaller indentation around it, so guides run through it.
    The step is the document's smallest indentation (1 to 8 columns), so a
    file indented by 4 gets a guide every 4 columns.
  - **A ruler:** a line after a column, from 10 to 500; none by default.
  - **Whitespace:** spaces as dots and tabs as arrows: never, in selected
    text (the default), or always.
  - **The cursor's line:** outlined (the default), tinted (nuvui's own),
    both, or nothing. The outline shows while the editor has focus.

  The guides and the ruler are markers in a CodeMirror layer, which places
  them through the CSSOM. The whitespace marks are classes whose text comes
  from a `data-` attribute. Nothing sets a style attribute or loads an image,
  so the strict CSP holds.
- `DiffSurface`: read only (a review), side by side or in one column when
  narrow, unchanged lines folded away; the language from the extension, for
  the ones the editor colors. Loaded lazily too.

### Known limit

`CodeEditor` keeps the state of every document it has shown and has no way to
forget one. A closed file's state stays in memory until the editor unmounts:
when the workspace changes, or when no text file is open. Worth raising with
nuvui (a way to drop a path).

## Public API

- `EditorSurface` with `EditorSurfaceProps`: `documentId`, `path`,
  `languageId?`, `initialValue`, `readOnly`, `onChange(value)`, `onSave()`,
  `diagnostics?`, `language?`, `breakpoints?`, `onToggleBreakpoint?`,
  `executionLine?`, `display?` (`SurfaceDisplay`: `indentGuides`,
  `rulerColumn`, `whitespace`, `lineHighlight`), `aria-label` (required),
  `styleNonce?`, `ref`.
- `SurfaceLanguage` (`completions`, `hover`, `goToDefinition`), `SurfacePosition`,
  `SurfaceCompletion`.
- `EditorSurfaceHandle` (through `ref`): `getValue()`, `setValue(value)` (calls
  `onChange`, and can be undone), `focus()`, `revealPosition(line, column)`
  (both from 1, clamped).
- `SurfaceDiagnostic`.
- `DiffSurface` with `DiffSurfaceProps`: `path`, `original`, `modified`,
  `aria-label` (required), `styleNonce?`.

## May depend on

`react`, `@nuvui/react`, `@nuvui/editor` (its `/code` entry only: the package's
root pulls in Tiptap, and that import fails lint). Never another feature
package, `@repo/ipc` or `@tauri-apps/*`.
