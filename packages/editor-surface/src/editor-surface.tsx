import "@nuvui/editor/css/code-editor.css";
import "./editor-surface.scss";
import {
  CodeEditor,
  CodeEditorContent,
  type CodeEditorHandle,
  CodeEditorStatus,
} from "@nuvui/editor/code";
import { type KeyboardEvent, useImperativeHandle, useMemo, useRef } from "react";
import type { EditorSurfaceProps, SurfaceLanguage } from "./contract";
import { languageExtensions } from "./language";

const isSave = (event: KeyboardEvent): boolean =>
  (event.ctrlKey || event.metaKey) &&
  !event.altKey &&
  !event.shiftKey &&
  event.key.toLowerCase() === "s";

const clamp = (value: number, min: number, max: number): number =>
  Math.min(Math.max(min, Math.trunc(value)), max);

/** How many frames `focus` and `revealPosition` wait for a new editor to be drawn. */
const READY_FRAMES = 60;

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
  language,
  "aria-label": label,
  styleNonce,
  ref,
}: EditorSurfaceProps) {
  const editor = useRef<CodeEditorHandle>(null);
  // The extensions are made once; they ask for the language features of the
  // document shown now, which change with it.
  const currentLanguage = useRef<SurfaceLanguage | undefined>(language);
  currentLanguage.current = language;
  const extensions = useMemo(() => languageExtensions(() => currentLanguage.current), []);

  useImperativeHandle(ref, () => {
    // The editor draws its view a render after it mounts, so a document opened
    // a moment ago may not be shown yet: wait for it, a frame at a time.
    const whenDrawn = (run: (handle: CodeEditorHandle) => void) => {
      let frames = 0;
      const attempt = () => {
        const handle = editor.current;
        if (handle?.view) run(handle);
        else if (++frames < READY_FRAMES) requestAnimationFrame(attempt);
      };
      attempt();
    };
    return {
      getValue: () => editor.current?.getValue() ?? "",
      setValue: (value) => editor.current?.setValue(value),
      focus: () => whenDrawn((handle) => handle.focus()),
      revealPosition: (line, column) =>
        whenDrawn((handle) => {
          const doc = handle.view?.state.doc;
          if (!doc) return;
          const target = doc.line(clamp(line, 1, doc.lines));
          handle.select(target.from + clamp(column - 1, 0, target.length));
        }),
    };
  }, []);

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
      extensions={extensions}
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
