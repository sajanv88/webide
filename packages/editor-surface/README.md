# @repo/editor-surface

**Status:** Phase 4.

## Purpose

The `EditorSurface` contract, and its implementation on nuvui's `CodeEditor`
(`@nuvui/editor/code`, which runs CodeMirror). This is the only package that
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

### Known limit

`CodeEditor` keeps the state of every document it has shown and has no way to
forget one. A closed file's state stays in memory until the editor unmounts:
when the workspace changes, or when no text file is open. Worth raising with
nuvui (a way to drop a path).

## Public API

- `EditorSurface` with `EditorSurfaceProps`: `documentId`, `path`,
  `languageId?`, `initialValue`, `readOnly`, `onChange(value)`, `onSave()`,
  `diagnostics?` (reserved), `aria-label` (required), `styleNonce?`, `ref`.
- `EditorSurfaceHandle` (through `ref`): `getValue()`, `setValue(value)` (calls
  `onChange`, and can be undone), `focus()`, `revealPosition(line, column)`
  (both from 1, clamped).
- `SurfaceDiagnostic`.

## May depend on

`react`, `@nuvui/react`, `@nuvui/editor` (its `/code` entry only: the package's
root pulls in Tiptap, and that import fails lint). Never another feature
package, `@repo/ipc` or `@tauri-apps/*`.
