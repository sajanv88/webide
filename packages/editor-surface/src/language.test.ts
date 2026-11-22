import { Text } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import { hoverContent, positionAt } from "./language";

describe("language extensions", () => {
  it("turns an offset into a 0-based line and a UTF-16 column", () => {
    const doc = Text.of(["const a = 1;", "let é😀b;"]);
    expect(positionAt(doc, 0)).toEqual({ line: 0, character: 0 });
    // Line 2 starts at 13; "let é😀" is 7 UTF-16 units.
    expect(positionAt(doc, 20)).toEqual({ line: 1, character: 7 });
    expect(positionAt(doc, 999)).toEqual({ line: 1, character: doc.line(2).length });
  });

  it("draws a hover's Markdown as text, never as HTML", () => {
    const dom = hoverContent("```typescript\nconst x: number\n```\nThe <b>count</b>.");
    expect([...dom.children].map((child) => child.tagName)).toEqual(["PRE", "P"]);
    expect(dom.querySelector("pre")?.textContent).toBe("const x: number");
    expect(dom.querySelector("p")?.textContent).toBe("The <b>count</b>.");
    expect(dom.querySelector("b")).toBeNull();
  });
});
