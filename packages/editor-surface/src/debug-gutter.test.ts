import { EditorState } from "@codemirror/state";
import { EditorView, runScopeHandlers } from "@codemirror/view";
import { afterEach, describe, expect, it, vi } from "vitest";
import { debugExtensions, setDebugMarks } from "./debug-gutter";

const views: EditorView[] = [];
afterEach(() => {
  for (const view of views.splice(0)) view.destroy();
});

function editor(toggle: ((line: number) => void) | undefined) {
  const view = new EditorView({
    state: EditorState.create({
      doc: "one\ntwo\nthree\nfour\n",
      extensions: debugExtensions(() => toggle),
    }),
    parent: document.body,
  });
  views.push(view);
  return view;
}

/** The lines (1-based) with a breakpoint dot, and the one marked paused. */
function marks(view: EditorView) {
  const lines = [...view.contentDOM.querySelectorAll(".cm-line")];
  return {
    dots: view.dom.querySelectorAll(
      ".editor-surface-breakpoints .cm-gutterElement:not([style*='hidden']) .editor-surface-breakpoint",
    ).length,
    paused: lines.findIndex((line) => line.classList.contains("editor-surface-execution")) + 1,
  };
}

const f9 = (view: EditorView) =>
  runScopeHandlers(view, new KeyboardEvent("keydown", { key: "F9" }), "editor");

describe("debug gutter", () => {
  it("shows the breakpoints and the paused line it's given", () => {
    const view = editor(vi.fn());
    view.dispatch({ effects: setDebugMarks.of({ breakpoints: [2, 4, 99], execution: 3 }) });
    expect(marks(view)).toEqual({ dots: 2, paused: 3 });
    view.dispatch({ effects: setDebugMarks.of({ breakpoints: [], execution: null }) });
    expect(marks(view)).toEqual({ dots: 0, paused: 0 });
  });

  it("moves its marks with the text", () => {
    const view = editor(vi.fn());
    view.dispatch({ effects: setDebugMarks.of({ breakpoints: [2], execution: 2 }) });
    view.dispatch({ changes: { from: 0, insert: "zero\n" } });
    expect(marks(view).paused).toBe(3);
  });

  it("F9 toggles the cursor's line, when breakpoints can be set", () => {
    const toggle = vi.fn();
    const view = editor(toggle);
    view.dispatch({ selection: { anchor: view.state.doc.line(3).from + 1 } });
    expect(f9(view)).toBe(true);
    expect(toggle).toHaveBeenCalledWith(3);

    const plain = editor(undefined);
    expect(f9(plain)).toBe(false);
  });
});
