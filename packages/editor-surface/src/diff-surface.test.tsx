import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { DiffSurface, languageOf } from "./diff-surface";

describe("DiffSurface", () => {
  it("takes the language from the file's extension, or none", () => {
    expect(languageOf("src/app.TS")).toBe("ts");
    expect(languageOf("styles/site.scss")).toBe("scss");
    expect(languageOf("Makefile")).toBeUndefined();
    expect(languageOf(".gitignore")).toBeUndefined();
    expect(languageOf("notes.weird")).toBeUndefined();
  });

  it("is named for what changed, and shows both texts", async () => {
    render(
      <DiffSurface
        path="src/a.ts"
        original={"const a = 1;\n"}
        modified={"const a = 2;\n"}
        aria-label="Changes to src/a.ts"
      />,
    );
    const diff = await screen.findByLabelText("Changes to src/a.ts");
    expect(diff.textContent).toContain("const a = 1;");
    expect(diff.textContent).toContain("const a = 2;");
  });
});
