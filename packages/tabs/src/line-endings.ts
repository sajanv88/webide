import type { LineEndingDto } from "@repo/contracts";

/**
 * The text as an editor holds it: every line break is `\n`. (Editors split
 * lines on `\r\n`, `\r` and `\n` and join them with `\n`, so a document keeps
 * this form and `toDiskText` puts the file's own line endings back.)
 */
export function toEditorText(text: string): string {
  return text.replace(/\r\n?/g, "\n");
}

/** The text to write: with `\r\n` again when the file had it. */
export function toDiskText(text: string, ending: LineEndingDto): string {
  return ending === "crlf" ? text.replace(/\n/g, "\r\n") : text;
}

/**
 * The file's line ending once `text` has been written with `toDiskText`.
 * A file with mixed endings, or none yet, is written with `\n`.
 */
export function endingAfterSave(ending: LineEndingDto, text: string): LineEndingDto {
  if (ending === "crlf" || ending === "lf") return ending;
  return text.includes("\n") ? "lf" : "none";
}
