// Edits from a language server (line and UTF-16 column), as CodeMirror changes.
import type { Text } from "@codemirror/state";
import type { SurfaceEdit, SurfacePosition } from "./contract";

/** A line and a UTF-16 column as a character offset, kept inside the text. */
export function offsetAt(doc: Text, at: SurfacePosition): number {
  if (at.line >= doc.lines) return doc.length;
  const line = doc.line(Math.max(0, at.line) + 1);
  return line.from + Math.min(Math.max(0, at.character), line.length);
}

/** One change per edit, by offset in `doc`: the text the edits were made against. */
export function editChanges(
  doc: Text,
  edits: readonly SurfaceEdit[],
): { from: number; to: number; insert: string }[] {
  return edits.map((edit) => {
    const from = offsetAt(doc, edit.from);
    return { from, to: Math.max(from, offsetAt(doc, edit.to)), insert: edit.insert };
  });
}

/** Whether changes overlap: then they can't be made together. */
export function overlapping(changes: readonly { from: number; to: number }[]): boolean {
  const sorted = [...changes].sort((a, b) => a.from - b.from || a.to - b.to);
  return sorted.some((change, index) => {
    const next = sorted[index + 1];
    return next !== undefined && next.from < change.to;
  });
}
