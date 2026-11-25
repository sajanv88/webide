# @repo/file-icons

**Status:** shared building block.

## Purpose

A file's icon by its type, the same wherever a file is shown: the explorer,
the editor tabs, search results and the git change list.

- `FileTypeIcon`: nuvui's file type icon (`@nuvui/icons/file-types`: a page
  with the extension on a colored tag) where there's one for the file,
  nuvui's plain file icon otherwise. 16 pixels unless `size` says otherwise.
  It's hidden from screen readers, as it always stands beside the file's
  name, which has the extension in it.
- Types: the 24 nuvui has (ts, tsx, js, jsx, json, html, css, scss, md, py,
  go, java, rs, c, cpp, cs, php, rb, sh, sql, yml, xml, vue, env), and the
  other spellings of the same types: `mts`/`cts`, `mjs`/`cjs`, `jsonc`,
  `json5`, `htm`, `markdown`, `pyi`, `h` (C), `cc`/`cxx`/`hpp`/`hh`/`hxx`
  (C++), `bash`/`zsh`, `yaml`. `.env`, `.env.local` and the like are `env`.
  The extension is matched in any case.
- The icon carries `data-file-type` (the extension, or `file`), for tests.

## Public API

- `FileTypeIcon` with `{ name, size?, className? }`: `name` is the file's
  name or path.
- `fileTypeOf(name)`: the extension that picks the icon, in lower case, or
  `undefined`.

## May depend on

`react`, `@nuvui/icons`. No other `@repo/*` package, `@nuvui/react` or
`@tauri-apps/*`. Feature packages may depend on it; foundation packages and
`@repo/ipc` may not. Checked by Biome and `turbo boundaries` (tag `ui`).
