import type { AppCommandId, AppEvents, IpcResult, LanguagePort, ProblemDto } from "@repo/contracts";
import { createCommandRegistry, createEventBus } from "@repo/kernel";
import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CHANGE_DELAY_MS, createLanguageClient, placeProblems } from "./language-client";
import { LanguageStatusItem } from "./language-status";

const ok = <T,>(value: T): IpcResult<T> => ({ ok: true, value });

const problem = (line: number, start: number, end: number): ProblemDto => ({
  range: { start: { line, character: start }, end: { line, character: end } },
  severity: "error",
  message: "Nope.",
  source: "ts",
  code: "2322",
});

function fakePort(overrides: Partial<LanguagePort> = {}) {
  const calls: string[] = [];
  const port: LanguagePort = {
    open: async (path, text) => {
      calls.push(`open ${path} ${text}`);
      return ok(null);
    },
    change: async (path, text) => {
      calls.push(`change ${path} ${text}`);
      return ok(null);
    },
    close: async (path) => {
      calls.push(`close ${path}`);
      return ok(null);
    },
    problems: async () => ok([problem(1, 6, 11)]),
    completions: async (path, line, character) => {
      calls.push(`complete ${path} ${line}:${character}`);
      return ok({
        items: [
          { label: "toFixed", kind: "method", detail: "(): string", insert: null, sort: null },
        ],
        incomplete: false,
      });
    },
    hover: async () => ok({ markdown: "const x: number" }),
    definition: async () =>
      ok([
        {
          kind: "workspace",
          path: "src/b.ts",
          range: { start: { line: 2, character: 4 }, end: { line: 2, character: 9 } },
        },
      ]),
    restart: async () => {
      calls.push("restart");
      return ok(null);
    },
    ...overrides,
  };
  return { port, calls };
}

describe("placeProblems", () => {
  it("turns lines and UTF-16 columns into offsets in the text", () => {
    const text = "const a = 1;\nlet é😀b = 'x';\n";
    const [placed] = placeProblems(text, [problem(1, 7, 8)]);
    // Line 2 starts at 13; "let " is 4 units and "é😀" 3 more, so column 7 is "b".
    expect(placed).toMatchObject({ from: 20, to: 21, line: 2, column: 8, severity: "error" });
    expect(text.slice(placed?.from, placed?.to)).toBe("b");
  });

  it("keeps a range past the end of its line inside the text", () => {
    const [placed] = placeProblems("ab", [problem(5, 0, 99)]);
    expect(placed?.from).toBeLessThanOrEqual(2);
    expect(placed?.to).toBeLessThanOrEqual(2);
  });
});

describe("createLanguageClient", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("opens script files, sends changes once typing pauses, and reports problems", async () => {
    const { port, calls } = fakePort();
    const bus = createEventBus<AppEvents>();
    const seen: AppEvents["language.problems"][] = [];
    bus.on("language.problems", (event) => seen.push(event));
    const client = createLanguageClient(port, bus);

    bus.emit("document.opened", { path: "notes.md", text: "x" });
    bus.emit("document.opened", { path: "src/a.ts", text: "one\nconst count = 1;" });
    await vi.runAllTimersAsync();
    expect(calls).toEqual(["open src/a.ts one\nconst count = 1;"]);
    expect(seen.at(-1)?.problems[0]).toMatchObject({ from: 10, to: 15, line: 2 });
    expect(client.problems("src/a.ts")).toHaveLength(1);

    bus.emit("document.changed", { path: "src/a.ts", text: "two" });
    bus.emit("document.changed", { path: "src/a.ts", text: "three" });
    await vi.advanceTimersByTimeAsync(CHANGE_DELAY_MS + 10);
    // Only the last text, once.
    expect(calls.filter((call) => call.startsWith("change"))).toEqual(["change src/a.ts three"]);

    bus.emit("document.closed", { path: "src/a.ts" });
    await vi.runAllTimersAsync();
    expect(calls.at(-1)).toBe("close src/a.ts");
    expect(seen.at(-1)).toEqual({ path: "src/a.ts", problems: [] });
    client.dispose();
  });

  it("sends pending text before asking for completions, and goes to definitions", async () => {
    const { port, calls } = fakePort();
    const bus = createEventBus<AppEvents>();
    const opened = vi.fn();
    bus.on("file.openRequested", opened);
    const client = createLanguageClient(port, bus);
    bus.emit("document.opened", { path: "src/a.ts", text: "x" });
    await vi.runAllTimersAsync();
    bus.emit("document.changed", { path: "src/a.ts", text: "x." });

    const features = client.features("src/a.ts");
    expect(client.features("src/a.ts")).toBe(features);
    const items = await features.completions({ line: 0, character: 2 });
    expect(calls.slice(-2)).toEqual(["change src/a.ts x.", "complete src/a.ts 0:2"]);
    expect(items).toEqual([{ label: "toFixed", kind: "method", detail: "(): string" }]);
    expect(await features.hover({ line: 0, character: 0 })).toBe("const x: number");

    features.goToDefinition({ line: 0, character: 0 });
    await vi.runAllTimersAsync();
    expect(opened).toHaveBeenCalledWith({ path: "src/b.ts", line: 3, column: 5 });
    client.dispose();
  });

  it("starts the server again with the open files", async () => {
    const { port, calls } = fakePort();
    const bus = createEventBus<AppEvents>();
    const client = createLanguageClient(port, bus);
    bus.emit("document.opened", { path: "a.ts", text: "1" });
    await vi.runAllTimersAsync();
    await client.restart();
    expect(calls).toEqual(["open a.ts 1", "restart", "open a.ts 1"]);
    client.dispose();
  });
});

describe("LanguageStatusItem", () => {
  it("shows the server starting, ready with the problems, or failed", () => {
    const { port } = fakePort();
    const bus = createEventBus<AppEvents>();
    const registry = createCommandRegistry<AppCommandId>();
    const client = createLanguageClient(port, bus);
    render(<LanguageStatusItem client={client} registry={registry} bus={bus} />);
    expect(registry.get("language.restart")).toBeTruthy();

    act(() => bus.emit("language.status", { state: "starting" }));
    expect(screen.getByText("TypeScript")).toBeTruthy();
    act(() => {
      bus.emit("language.status", { state: "ready" });
      bus.emit("language.problems", {
        path: "a.ts",
        problems: [{ from: 0, to: 1, severity: "error", message: "x", line: 1, column: 1 }],
      });
    });
    expect(
      screen.getByRole("status", { name: "TypeScript ready. 1 error, 0 warnings in open files" }),
    ).toBeTruthy();
    act(() => bus.emit("language.status", { state: "failed", message: "no server" }));
    expect(
      screen.getByRole("button", { name: "TypeScript isn't running: no server. Restart it" }),
    ).toBeTruthy();
    client.dispose();
  });
});
