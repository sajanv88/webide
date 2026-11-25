// The EditorSurface contract: all the app knows about the editor. Which
// editor implements it is this package's business only.
import type { Ref } from "react";

/** A problem to mark in the text. */
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
  /** Problems to mark in the text, underlined and in the gutter. */
  readonly diagnostics?: readonly SurfaceDiagnostic[];
  /** Completion, hover and go to definition, from a language server. */
  readonly language?: SurfaceLanguage | undefined;
  /** Lines (1-based) with a breakpoint, marked in a gutter. */
  readonly breakpoints?: readonly number[] | undefined;
  /** A press in the gutter, or F9: toggles that line's breakpoint. Without it, there are none. */
  readonly onToggleBreakpoint?: ((line: number) => void) | undefined;
  /** The line (1-based) the debugger is paused on, highlighted. */
  readonly executionLine?: number | null | undefined;
  /** How the lines are drawn: guides, a ruler, whitespace, the cursor's line. */
  readonly display?: SurfaceDisplay | undefined;
  /** What a screen reader calls the text, such as the file's path. Required. */
  readonly "aria-label": string;
  /** The page's CSP nonce, for the styles the editor adds as it runs. */
  readonly styleNonce?: string | undefined;
  readonly ref?: Ref<EditorSurfaceHandle>;
}

/** How the editor draws the text's lines. */
export interface SurfaceDisplay {
  /** A thin line down each level of indentation (blank lines included). */
  readonly indentGuides: boolean;
  /** A line down the editor after this many columns; `null` for none. */
  readonly rulerColumn: number | null;
  /** Spaces as dots and tabs as arrows: never, in selected text, or always. */
  readonly whitespace: "none" | "selection" | "all";
  /** The cursor's line: unmarked, tinted, outlined, or both. */
  readonly lineHighlight: "none" | "tint" | "border" | "both";
}

/** A place in the text: 0-based line, and column in UTF-16 units. */
export interface SurfacePosition {
  readonly line: number;
  readonly character: number;
}

/** One suggestion while typing. */
export interface SurfaceCompletion {
  readonly label: string;
  /** What kind of thing: `method`, `function`, `variable`, `class`, `keyword`… */
  readonly kind?: string;
  /** A type or signature, shown beside it. */
  readonly detail?: string;
  /** What's inserted, when it isn't the label. */
  readonly insert?: string;
}

/**
 * Language features for the document shown, answered by its owner (a
 * language server). Any answer may come back `null`: nothing to say.
 */
export interface SurfaceLanguage {
  completions(at: SurfacePosition): Promise<readonly SurfaceCompletion[] | null>;
  /** Markdown. */
  hover(at: SurfacePosition): Promise<string | null>;
  /** Goes to where the thing at `at` is defined (F12, or Ctrl/Cmd and a click). */
  goToDefinition(at: SurfacePosition): void;
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

/** Two versions of a file, side by side (or in one column when narrow), to review. */
export interface DiffSurfaceProps {
  /** Workspace-relative, `/`-separated. The language comes from its extension. */
  readonly path: string;
  /** The text before; `""` for a file that's new. */
  readonly original: string;
  /** The text after; `""` for a file that's gone. */
  readonly modified: string;
  /** What changed, and in which file, for a screen reader. Required. */
  readonly "aria-label": string;
  /** The page's CSP nonce, for the styles the editor adds as it runs. */
  readonly styleNonce?: string | undefined;
}
