import type { Completion } from "@codemirror/autocomplete";
import { EditorState, Text } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it } from "vitest";
import type { SurfaceEdit, SurfaceFix, SurfaceLanguage } from "./contract";
import { editChanges, offsetAt, overlapping } from "./edits";
import { applyCompletion } from "./language";
import { applyFix, fixMenuExtension, fixMenuOpen, orderFixes, showFixes } from "./quick-fix";

const IMPORT = 'import { useState } from "react";\n';
const atTop: SurfaceEdit = {
  from: { line: 0, character: 0 },
  to: { line: 0, character: 0 },
  insert: IMPORT,
};

const views: EditorView[] = [];
const viewOf = (doc: string) => {
  const parent = document.createElement("div");
  document.body.append(parent);
  const view = new EditorView({
    state: EditorState.create({ doc, extensions: fixMenuExtension }),
    parent,
  });
  views.push(view);
  return view;
};
afterEach(() => {
  for (const view of views.splice(0)) {
    view.dom.parentElement?.remove();
    view.destroy();
  }
});
const picked = (label: string): Completion => ({ label });

describe("edits from a language server", () => {
  it("places lines and UTF-16 columns in the text, inside it", () => {
    const doc = Text.of(["let a;", "let é😀b;"]);
    expect(offsetAt(doc, { line: 1, character: 6 })).toBe(13);
    expect(offsetAt(doc, { line: 0, character: 99 })).toBe(6);
    expect(offsetAt(doc, { line: 9, character: 0 })).toBe(doc.length);
    expect(editChanges(doc, [atTop])).toEqual([{ from: 0, to: 0, insert: IMPORT }]);
    expect(
      overlapping([
        { from: 0, to: 4 },
        { from: 2, to: 6 },
      ]),
    ).toBe(true);
    expect(
      overlapping([
        { from: 0, to: 2 },
        { from: 2, to: 6 },
      ]),
    ).toBe(false);
  });
});

describe("picking a completion", () => {
  it("adds the import it comes with, and leaves the cursor after the name", async () => {
    const view = viewOf("const [n] = useSta");
    const item = { label: "useState", resolve: async () => [atTop] };
    applyCompletion(item)(view, picked("useState"), 12, 18);
    expect(view.state.doc.toString()).toBe("const [n] = useState");
    await Promise.resolve();
    await Promise.resolve();
    expect(view.state.doc.toString()).toBe(`${IMPORT}const [n] = useState`);
    expect(view.state.selection.main.head).toBe(view.state.doc.length);
  });

  it("makes edits that came with the item at once", () => {
    const view = viewOf("useSta");
    applyCompletion({ label: "useState", edits: [atTop] })(view, picked("useState"), 0, 6);
    expect(view.state.doc.toString()).toBe(`${IMPORT}useState`);
  });

  it("drops a late import when the text before it changed meanwhile", async () => {
    const start = 'import { useMemo } from "react";\n';
    const view = viewOf(`${start}useSta`);
    let answer: (edits: SurfaceEdit[]) => void = () => {};
    const item = {
      label: "useState",
      resolve: () => new Promise<SurfaceEdit[]>((resolve) => (answer = resolve)),
    };
    applyCompletion(item)(view, picked("useState"), start.length, start.length + 6);
    // Typed before the import line: its places have moved.
    view.dispatch({ changes: { from: 0, insert: "// note\n" } });
    const at = { line: 0, character: 9 };
    answer([{ from: at, to: at, insert: "useState, " }]);
    await Promise.resolve();
    await Promise.resolve();
    expect(view.state.doc.toString()).toBe(`// note\n${start}useState`);
  });
});

describe("quick fixes", () => {
  const fixes: SurfaceFix[] = [
    { title: "Remove unused declaration", preferred: false, edits: [] },
    { title: 'Add import from "react"', preferred: true, edits: [atTop] },
  ];
  const language = (found: readonly SurfaceFix[] | null): SurfaceLanguage => ({
    completions: async () => null,
    hover: async () => null,
    goToDefinition: () => {},
    fixes: async () => found,
  });

  it("puts the preferred fix first", () => {
    expect(orderFixes(fixes).map((fix) => fix.title)).toEqual([
      'Add import from "react"',
      "Remove unused declaration",
    ]);
  });

  it("makes a fix only on the text it was made for", () => {
    const view = viewOf("useState(0);");
    const doc = view.state.doc;
    view.dispatch({ changes: { from: 0, insert: " " } });
    expect(applyFix(view, fixes[1] as SurfaceFix, doc)).toBe(false);
    expect(applyFix(view, fixes[1] as SurfaceFix, view.state.doc)).toBe(true);
    expect(view.state.doc.toString()).toBe(`${IMPORT} useState(0);`);
  });

  it("shows the fixes as a menu at the cursor, and makes the one picked", async () => {
    const view = viewOf("useState(0);");
    await showFixes(view, language(fixes));
    const menu = view.dom.querySelector('[role="menu"]');
    expect(menu?.getAttribute("aria-label")).toBe("Quick fixes");
    const items = [...(menu?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]') ?? [])];
    expect(items.map((item) => item.textContent)).toEqual([
      'Add import from "react"',
      "Remove unused declaration",
    ]);
    items[0]?.click();
    expect(view.state.doc.toString()).toBe(`${IMPORT}useState(0);`);
    expect(fixMenuOpen(view.state)).toBe(false);
  });

  it("closes with Escape, and says so when there are none", async () => {
    const view = viewOf("let a = 1;");
    await showFixes(view, language(fixes));
    const menu = view.dom.querySelector('[role="menu"]');
    menu?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(fixMenuOpen(view.state)).toBe(false);

    // Focus going elsewhere closes it too, once the editor's update is over.
    await showFixes(view, language(fixes));
    view.dom.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
    view.contentDOM.focus();
    await new Promise((resolve) => setTimeout(resolve));
    expect(fixMenuOpen(view.state)).toBe(false);

    await showFixes(view, language([]));
    expect(view.dom.querySelector('[role="status"]')?.textContent).toBe("No quick fixes here");
    view.contentDOM.dispatchEvent(new KeyboardEvent("keydown", { key: "a", bubbles: true }));
    expect(fixMenuOpen(view.state)).toBe(false);
  });
});
