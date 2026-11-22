import "@nuvui/editor/css/code-editor.css";
import "@nuvui/editor/css/diff-editor.css";
import "./diff-surface.scss";
import { DiffEditor } from "@nuvui/editor/code";
import type { DiffSurfaceProps } from "./contract";

/** Extensions the diff editor colors; anything else is plain text. */
const LANGUAGES = new Set([
  "ts",
  "tsx",
  "js",
  "jsx",
  "mjs",
  "cjs",
  "mts",
  "cts",
  "json",
  "css",
  "scss",
  "html",
  "md",
  "py",
  "sql",
  "yaml",
  "yml",
]);

/** The language of `path`, by its extension, when the editor knows it. */
export function languageOf(path: string): string | undefined {
  const name = path.slice(path.lastIndexOf("/") + 1);
  const dot = name.lastIndexOf(".");
  const extension = dot > 0 ? name.slice(dot + 1).toLowerCase() : "";
  return LANGUAGES.has(extension) ? extension : undefined;
}

/** nuvui's DiffEditor, read only: a review, nothing to keep or undo. */
export function DiffSurface({
  path,
  original,
  modified,
  "aria-label": label,
  styleNonce,
}: DiffSurfaceProps) {
  const language = languageOf(path);
  return (
    <DiffEditor
      className="diff-surface"
      aria-label={label}
      original={original}
      modified={modified}
      {...(language ? { language } : {})}
      {...(styleNonce ? { nonce: styleNonce } : {})}
    />
  );
}
