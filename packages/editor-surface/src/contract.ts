// The EditorSurface contract: all the app knows about the editor. Which
// editor implements it is this package's business only.
import type { Ref } from "react";

/** A problem to mark in the text. Reserved: nothing reports problems yet. */
export interface SurfaceDiagnostic {
  /** Characters from the start of the text. */
  readonly from: number;
  readonly to: number;
  readonly severity: "error" | "warning" | "info" | "hint";
  readonly message: string;
  /** What found it, such as "TypeScript". */
  readonly source?: string;
}

export interface EditorSurfaceProps {
  /**
   * One open document. The surface keeps each document's own undo history,
   * selection and scroll, and shows it again as it was left. A new id is a
   * new document, even for a path that was open before.
   */
  readonly documentId: string;
  /** Workspace-relative, `/`-separated. The language comes from its extension. */
  readonly path: string;
  /** A language by name or extension (`"typescript"`, `"ts"`), when the path doesn't say. */
  readonly languageId?: string;
  /**
   * The text, read the first time `documentId` is shown. Later changes to
   * this prop are ignored: the surface is uncontrolled, use `setValue`.
   * Line breaks are `\n`.
   */
  readonly initialValue: string;
  readonly readOnly: boolean;
  /** Called with the whole text after each change made in the editor. */
  readonly onChange: (value: string) => void;
  /** Ctrl+S, or Cmd+S on macOS, in the editor. */
  readonly onSave: () => void;
  /** Reserved. */
  readonly diagnostics?: readonly SurfaceDiagnostic[];
  /** What a screen reader calls the text, such as the file's path. Required. */
  readonly "aria-label": string;
  /** The page's CSP nonce, for the styles the editor adds as it runs. */
  readonly styleNonce?: string | undefined;
  readonly ref?: Ref<EditorSurfaceHandle>;
}

/** The editor from outside, for the document it shows now. */
export interface EditorSurfaceHandle {
  getValue(): string;
  /** Replaces the text, as a reload from disk would. Calls `onChange`, and can be undone. */
  setValue(value: string): void;
  /** Focuses the editor; for one just shown, once it's drawn. */
  focus(): void;
  /**
   * Puts the cursor at a line and column, both from 1 (the column in UTF-16
   * units), scrolls to it and focuses; for a document just shown, once it's drawn.
   */
  revealPosition(line: number, column: number): void;
}
