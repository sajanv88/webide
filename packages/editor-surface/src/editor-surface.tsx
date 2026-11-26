import "@nuvui/editor/css/code-editor.css";
import "./editor-surface.scss";
import {
  CodeEditor,
  CodeEditorContent,
  type CodeEditorHandle,
  CodeEditorStatus,
} from "@nuvui/editor/code";
import { type KeyboardEvent, useEffect, useImperativeHandle, useMemo, useRef } from "react";
import type { EditorSurfaceProps, SurfaceLanguage } from "./contract";
import { debugExtensions, setDebugMarks } from "./debug-gutter";
import { DEFAULT_DISPLAY, displayExtensions } from "./display";
import { editChanges, overlapping } from "./edits";
import { languageExtensions } from "./language";
import { fixMenuExtension, showFixes } from "./quick-fix";

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
  readOnly: readOnlyProp,
  onChange,
  onSave,
  diagnostics,
  language,
  breakpoints,
  onToggleBreakpoint,
  executionLine,
  display = DEFAULT_DISPLAY,
  "aria-label": label,
  styleNonce,
  ref,
}: EditorSurfaceProps) {
  const editor = useRef<CodeEditorHandle>(null);
  // The extensions are made once; they ask for the language features of the
  // document shown now, which change with it.
  const currentLanguage = useRef<SurfaceLanguage | undefined>(language);
  currentLanguage.current = language;
  const currentToggle = useRef(onToggleBreakpoint);
  currentToggle.current = onToggleBreakpoint;
  const readOnly = useRef(readOnlyProp);
  readOnly.current = readOnlyProp;
  const { indentGuides, rulerColumn, whitespace, lineHighlight } = display;
  // Made again only when the display changes: the editor then reconfigures them.
  const extensions = useMemo(
    () => [
      ...languageExtensions(() => currentLanguage.current),
      ...fixMenuExtension,
      ...debugExtensions(() => currentToggle.current),
      ...displayExtensions({ indentGuides, rulerColumn, whitespace, lineHighlight }),
    ],
    [indentGuides, rulerColumn, whitespace, lineHighlight],
  );

  // The document's breakpoints and paused line, put in the editor once it's
  // drawn (and again when it shows another document).
  const lines = (breakpoints ?? []).join(",");
  // biome-ignore lint/correctness/useExhaustiveDependencies: `lines` stands for `breakpoints`; `path` is which document is shown
  useEffect(() => {
    const marks = {
      breakpoints: lines ? lines.split(",").map(Number) : [],
      execution: executionLine ?? null,
    };
    let frames = 0;
    let request = 0;
    const apply = () => {
      const view = editor.current?.view;
      if (view) view.dispatch({ effects: setDebugMarks.of(marks) });
      else if (++frames < READY_FRAMES) request = requestAnimationFrame(apply);
    };
    apply();
    return () => cancelAnimationFrame(request);
  }, [lines, executionLine, path, documentId]);

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
      showFixes: () => {
        const view = editor.current?.view;
        if (view) void showFixes(view, currentLanguage.current);
      },
      runSourceAction: async (action) => {
        const view = editor.current?.view;
        const features = currentLanguage.current;
        if (!view || !features?.sourceAction || readOnly.current) return "unavailable";
        const doc = view.state.doc;
        const edits = await features.sourceAction(action);
        if (edits === null) return "unavailable";
        if (view.state.doc !== doc) return "stale";
        const changes = editChanges(doc, edits).filter(
          (change) => doc.sliceString(change.from, change.to) !== change.insert,
        );
        if (changes.length === 0) return "unchanged";
        if (overlapping(changes)) return "unavailable";
        view.dispatch({ changes, userEvent: "input.source" });
        return view.state.doc.eq(doc) ? "unchanged" : "changed";
      },
    };
  }, []);

  return (
    <CodeEditor
      className={`editor-surface editor-surface--line-${lineHighlight}`}
      editorRef={editor}
      aria-label={label}
      // The editor keeps a document's state by its path. The id makes the key
      // new for each open document; the file's name at the end gives the language.
      path={`${documentId}/${path}`}
      // Read only when a document is first shown: the editor is uncontrolled.
      defaultValue={initialValue}
      onValueChange={(code) => onChange(code)}
      readOnly={readOnlyProp}
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
