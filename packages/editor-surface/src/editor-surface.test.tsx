import { act, fireEvent, render, screen } from "@testing-library/react";
import { createRef } from "react";
import { describe, expect, it, vi } from "vitest";
import type { EditorSurfaceHandle, EditorSurfaceProps } from "./contract";
import { EditorSurface } from "./editor-surface";

function setup(overrides: Partial<EditorSurfaceProps> = {}) {
  const ref = createRef<EditorSurfaceHandle>();
  const onChange = vi.fn();
  const onSave = vi.fn();
  const props: EditorSurfaceProps = {
    documentId: "doc-1",
    path: "src/app.ts",
    initialValue: "const a = 1;\nconst b = 2;\n",
    readOnly: false,
    onChange,
    onSave,
    "aria-label": "Text of src/app.ts",
    ref,
    ...overrides,
  };
  const view = render(<EditorSurface {...props} />);
  const rerender = (next: Partial<EditorSurfaceProps>) =>
    view.rerender(<EditorSurface {...props} {...next} />);
  return { ref, onChange, onSave, rerender };
}

const textbox = (name: string) => screen.getByRole("textbox", { name });

describe("EditorSurface", () => {
  it("is a textbox named by aria-label, holding the initial text", () => {
    const { ref } = setup();
    expect(textbox("Text of src/app.ts").getAttribute("aria-multiline")).toBe("true");
    expect(ref.current?.getValue()).toBe("const a = 1;\nconst b = 2;\n");
  });

  it("saves on Ctrl+S and on Cmd+S, and keeps the browser from handling it", () => {
    const { onSave } = setup();
    const box = textbox("Text of src/app.ts");
    expect(fireEvent.keyDown(box, { key: "s", ctrlKey: true })).toBe(false);
    fireEvent.keyDown(box, { key: "S", metaKey: true });
    fireEvent.keyDown(box, { key: "s", ctrlKey: true, shiftKey: true });
    expect(onSave).toHaveBeenCalledTimes(2);
  });

  it("reports setValue through onChange", () => {
    const { ref, onChange } = setup();
    act(() => ref.current?.setValue("let x;\n"));
    expect(onChange).toHaveBeenLastCalledWith("let x;\n");
    expect(ref.current?.getValue()).toBe("let x;\n");
  });

  it("keeps each document's text: a new id starts from its own text, and an old one comes back as it was", () => {
    const { ref, rerender } = setup();
    act(() => ref.current?.setValue("edited\n"));

    rerender({ documentId: "doc-2", path: "README.md", initialValue: "# Hello\n" });
    expect(ref.current?.getValue()).toBe("# Hello\n");

    rerender({ documentId: "doc-1", path: "src/app.ts", initialValue: "ignored now\n" });
    expect(ref.current?.getValue()).toBe("edited\n");
  });

  it("marks a read-only document", () => {
    setup({ readOnly: true });
    expect(textbox("Text of src/app.ts").getAttribute("aria-readonly")).toBe("true");
  });

  it("reveals a line and column, clamped to the text", () => {
    const { ref } = setup();
    const status = screen.getByRole("group", { name: "Code status" });
    act(() => ref.current?.revealPosition(2, 7));
    expect(status.textContent).toContain("Line 2, column 7");
    act(() => ref.current?.revealPosition(99, 99));
    // The last line is the empty one after the final line break.
    expect(status.textContent).toContain("Line 3, column 1");
  });
});
