import { EditorSelection, EditorState, Text } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it } from "vitest";
import type { SurfaceDisplay } from "./contract";
import { DEFAULT_DISPLAY, displayExtensions, guideLevel, indentStep, indentWidth } from "./display";

const views: EditorView[] = [];
afterEach(() => {
  for (const view of views.splice(0)) view.destroy();
});

function editor(doc: string, display: Partial<SurfaceDisplay>) {
  const view = new EditorView({
    state: EditorState.create({
      doc,
      extensions: displayExtensions({ ...DEFAULT_DISPLAY, ...display }),
    }),
    parent: document.body,
  });
  views.push(view);
  return view;
}

const marks = (view: EditorView) =>
  [...view.contentDOM.querySelectorAll(".editor-surface-space, .editor-surface-tab")].map((mark) =>
    mark.classList.contains("editor-surface-tab") ? "tab" : mark.getAttribute("data-marks"),
  );

describe("indentation", () => {
  it("measures leading spaces and tabs in columns", () => {
    expect(indentWidth("    x", 4)).toBe(4);
    expect(indentWidth("\tx", 4)).toBe(4);
    expect(indentWidth("  \tx", 4)).toBe(4);
    expect(indentWidth("x", 2)).toBe(0);
    expect(indentWidth("   ", 2)).toBeNull();
    expect(indentWidth("", 2)).toBeNull();
  });

  it("finds the document's step, whatever the tab size", () => {
    expect(indentStep(Text.of(["a", "    b", "        c"]), 2)).toBe(4);
    expect(indentStep(Text.of(["a", "  b"]), 4)).toBe(2);
    expect(indentStep(Text.of(["a", "b"]), 4)).toBe(4);
  });

  it("gives a blank line the smaller indentation around it", () => {
    const doc = Text.of(["if (a) {", "  if (b) {", "    one();", "", "    two();", "  }", "", "}"]);
    expect([1, 2, 3, 4, 5, 6, 7, 8].map((line) => guideLevel(doc, line, 2, 2))).toEqual([
      0, 1, 2, 2, 2, 1, 0, 0,
    ]);
  });
});

describe("display extensions", () => {
  it("marks spaces and tabs everywhere with whitespace: all", () => {
    const view = editor("a  b\tc", { whitespace: "all" });
    expect(marks(view)).toEqual(["··", "tab"]);
  });

  it("marks only selected whitespace with whitespace: selection", () => {
    const view = editor("a b c", { whitespace: "selection" });
    expect(marks(view)).toEqual([]);
    view.dispatch({ selection: EditorSelection.range(0, 3) });
    expect(marks(view)).toEqual(["·"]);
  });

  it("marks nothing with whitespace: none", () => {
    expect(marks(editor("a b", { whitespace: "none" }))).toEqual([]);
  });

  it("outlines the cursor's line for border and both, not for tint", () => {
    const outlined = (view: EditorView) =>
      [...view.contentDOM.querySelectorAll(".cm-line")].findIndex((line) =>
        line.classList.contains("editor-surface-cursor-line"),
      );
    const view = editor("one\ntwo", { lineHighlight: "border" });
    expect(outlined(view)).toBe(0);
    view.dispatch({ selection: { anchor: 5 } });
    expect(outlined(view)).toBe(1);
    expect(outlined(editor("one", { lineHighlight: "both" }))).toBe(0);
    expect(outlined(editor("one", { lineHighlight: "tint" }))).toBe(-1);
  });

  it("adds a layer for the guides and the ruler only when they're on", () => {
    const layers = (view: EditorView) =>
      ["editor-surface-guides", "editor-surface-rulers"].filter((name) =>
        view.dom.querySelector(`.${name}`),
      );
    expect(layers(editor("  x", { indentGuides: true, rulerColumn: 80 }))).toEqual([
      "editor-surface-guides",
      "editor-surface-rulers",
    ]);
    expect(layers(editor("  x", { indentGuides: false, rulerColumn: null }))).toEqual([]);
  });
});
