import "@nuvui/editor/css/code-editor.css";
import "./editor-surface.scss";
import {
  CodeEditor,
  CodeEditorContent,
  type CodeEditorHandle,
  CodeEditorStatus,
} from "@nuvui/editor/code";
import { type KeyboardEvent, useImperativeHandle, useRef } from "react";
import type { EditorSurfaceProps } from "./contract";

const isSave = (event: KeyboardEvent): boolean =>
  (event.ctrlKey || event.metaKey) &&
  !event.altKey &&
  !event.shiftKey &&
  event.key.toLowerCase() === "s";

const clamp = (value: number, min: number, max: number): number =>
  Math.min(Math.max(min, Math.trunc(value)), max);

/**
 * The editor: nuvui's CodeEditor, with the code and the status line under it
 * (where the cursor is, and the language). One editor shows every document,
 * and keeps each one's undo history, selection and scroll.
 */
export function EditorSurface({
  documentId,
  path,
  languageId,
  initialValue,
  readOnly,
  onChange,
  onSave,
  diagnostics,
  "aria-label": label,
  styleNonce,
  ref,
}: EditorSurfaceProps) {
  const editor = useRef<CodeEditorHandle>(null);

  useImperativeHandle(
    ref,
    () => ({
      getValue: () => editor.current?.getValue() ?? "",
      setValue: (value) => editor.current?.setValue(value),
      focus: () => editor.current?.focus(),
      revealPosition(line, column) {
        const handle = editor.current;
        const doc = handle?.view?.state.doc;
        if (!handle || !doc) return;
        const target = doc.line(clamp(line, 1, doc.lines));
        handle.select(target.from + clamp(column - 1, 0, target.length));
      },
    }),
    [],
  );

  return (
    <CodeEditor
      className="editor-surface"
      editorRef={editor}
      aria-label={label}
      // The editor keeps a document's state by its path. The id makes the key
      // new for each open document; the file's name at the end gives the language.
      path={`${documentId}/${path}`}
      // Read only when a document is first shown: the editor is uncontrolled.
      defaultValue={initialValue}
      onValueChange={(code) => onChange(code)}
      readOnly={readOnly}
      {...(languageId ? { language: languageId } : {})}
      {...(diagnostics ? { diagnostics } : {})}
      {...(styleNonce ? { nonce: styleNonce } : {})}
      onKeyDown={(event) => {
        if (!isSave(event)) return;
        event.preventDefault();
        onSave();
      }}
    >
      <CodeEditorContent className="editor-surface__code" />
      <CodeEditorStatus className="editor-surface__status" />
    </CodeEditor>
  );
}
