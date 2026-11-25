import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { FileTypeIcon, fileTypeOf } from "./index";

afterEach(cleanup);

describe("fileTypeOf", () => {
  it("reads the extension, in any case", () => {
    expect(fileTypeOf("main.ts")).toBe("ts");
    expect(fileTypeOf("App.TSX")).toBe("tsx");
    expect(fileTypeOf("crates/ide-mcp/src/lib.rs")).toBe("rs");
    expect(fileTypeOf("archive.tar.json")).toBe("json");
  });

  it("knows the other spellings of a type", () => {
    expect(fileTypeOf("vite.config.mts")).toBe("mts");
    expect(fileTypeOf("ci.yaml")).toBe("yaml");
    expect(fileTypeOf("util.h")).toBe("h");
  });

  it("takes .env and its variants as env", () => {
    expect(fileTypeOf(".env")).toBe("env");
    expect(fileTypeOf(".env.local")).toBe("env");
    expect(fileTypeOf("prod.env")).toBe("env");
    expect(fileTypeOf(".environment")).toBeUndefined();
  });

  it("has nothing for other files", () => {
    expect(fileTypeOf("Dockerfile")).toBeUndefined();
    expect(fileTypeOf(".gitignore")).toBeUndefined();
    expect(fileTypeOf("photo.png")).toBeUndefined();
    expect(fileTypeOf("name.")).toBeUndefined();
    expect(fileTypeOf("x.constructor")).toBeUndefined();
  });
});

describe("FileTypeIcon", () => {
  it("draws the file type icon, hidden from screen readers", () => {
    const { container } = render(<FileTypeIcon name="index.tsx" className="row__icon" />);
    const svg = container.querySelector("svg");
    expect(svg?.getAttribute("data-file-type")).toBe("tsx");
    expect(svg?.getAttribute("aria-hidden")).toBe("true");
    expect(svg?.getAttribute("width")).toBe("16");
    expect(svg?.classList.contains("row__icon")).toBe(true);
  });

  it("falls back to the plain file icon", () => {
    const { container } = render(<FileTypeIcon name="LICENSE" size={14} />);
    const svg = container.querySelector("svg");
    expect(svg?.getAttribute("data-file-type")).toBe("file");
    expect(svg?.getAttribute("width")).toBe("14");
  });
});
