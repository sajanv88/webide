import type {
  AppCommandId,
  AppEvents,
  DebugPort,
  IpcResult,
  ScopeDto,
  StackFrameDto,
  VariableDto,
} from "@repo/contracts";
import { createCommandRegistry, createEventBus } from "@repo/kernel";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { createDebugStore, debuggable, MAX_OUTPUT } from "./debug-store";
import { DebugView } from "./debug-view";

const ok = <T,>(value: T): IpcResult<T> => ({ ok: true, value });
const done = ok(null);

const frames: StackFrameDto[] = [
  {
    id: 7,
    name: "inner",
    source: { kind: "outside", name: "node:internal/x" },
    line: 1,
    column: 1,
  },
  { id: 8, name: "total", source: { kind: "workspace", path: "m6/shop.js" }, line: 4, column: 0 },
  {
    id: 9,
    name: "<anonymous>",
    source: { kind: "workspace", path: "m6/shop.js" },
    line: 10,
    column: 3,
  },
];
const scopes: ScopeDto[] = [
  { name: "Local: total", reference: 20, expensive: false },
  { name: "Global", reference: 21, expensive: true },
];
const variables: Record<number, VariableDto[]> = {
  20: [
    { name: "sum", value: "0", kind: "number", reference: 0 },
    { name: "item", value: "{name: 'tea'}", kind: "object", reference: 30 },
  ],
  30: [{ name: "name", value: "'tea'", kind: "string", reference: 0 }],
};

/** A debugger in memory: records what it's asked. */
function fakePort(overrides: Partial<DebugPort> = {}) {
  const calls: string[] = [];
  const port: DebugPort = {
    start: async (path, breakpoints) => {
      calls.push(`start ${path} ${JSON.stringify(breakpoints)}`);
      return done;
    },
    stop: async () => {
      calls.push("stop");
      return done;
    },
    setBreakpoints: async (path, lines) => {
      calls.push(`breakpoints ${path} ${lines.join(",")}`);
      return ok(lines.map((line) => ({ line, verified: true, actualLine: null })));
    },
    resume: async (thread, step) => {
      calls.push(`resume ${thread}${step ? ` ${step}` : ""}`);
      return done;
    },
    pause: async (thread) => {
      calls.push(`pause ${thread}`);
      return done;
    },
    stack: async () => ok(frames),
    scopes: async (frame) => {
      calls.push(`scopes ${frame}`);
      return ok(scopes);
    },
    variables: async (reference) => {
      calls.push(`variables ${reference}`);
      return ok(variables[reference] ?? []);
    },
    evaluate: async (expression, frame) => {
      calls.push(`evaluate ${expression} ${frame}`);
      return expression === "oops"
        ? { ok: false, error: { code: "internal", message: "oops is not defined" } }
        : ok("103");
    },
    ...overrides,
  };
  return { port, calls };
}

function setup(overrides: Partial<DebugPort> = {}) {
  const { port, calls } = fakePort(overrides);
  const bus = createEventBus<AppEvents>();
  const opened: AppEvents["file.openRequested"][] = [];
  bus.on("file.openRequested", (request) => opened.push(request));
  const store = createDebugStore(port, bus);
  return { store, bus, calls, opened };
}

/** Pauses `store` as a breakpoint would. */
async function pause(
  bus: ReturnType<typeof setup>["bus"],
  store: ReturnType<typeof setup>["store"],
) {
  bus.emit("debug.event", {
    kind: "stopped",
    threadId: 0,
    reason: "breakpoint",
    description: null,
  });
  await waitFor(() => expect(store.getState().scopes.length).toBe(2));
}

describe("debuggable", () => {
  it("takes JavaScript and TypeScript files", () => {
    for (const path of ["a.js", "b.mjs", "c.cjs", "d.ts", "e.MTS", "src/f.cts"]) {
      expect(debuggable(path)).toBe(true);
    }
    for (const path of ["a.json", "b.tsx", "c.md", "js"]) expect(debuggable(path)).toBe(false);
  });
});

describe("createDebugStore", () => {
  it("keeps breakpoints sorted, and tells the debugger only while one runs", async () => {
    const { store, calls } = setup();
    store.toggleBreakpoint("a.js", 9);
    store.toggleBreakpoint("a.js", 2);
    store.toggleBreakpoint("b.js", 5);
    expect(store.getState().breakpoints.get("a.js")).toEqual([2, 9]);
    expect(calls).toEqual([]);

    await store.start("a.js");
    expect(store.getState().phase).toBe("running");
    expect(calls).toEqual([
      `start a.js ${JSON.stringify([
        { path: "a.js", lines: [2, 9] },
        { path: "b.js", lines: [5] },
      ])}`,
    ]);
    store.toggleBreakpoint("a.js", 9);
    store.removeBreakpoint("b.js", 5);
    expect(store.getState().breakpoints.get("a.js")).toEqual([2]);
    expect(store.getState().breakpoints.has("b.js")).toBe(false);
    expect(calls.slice(1)).toEqual(["breakpoints a.js 2", "breakpoints b.js "]);
  });

  it("says why it couldn't start", async () => {
    const { store } = setup({
      start: async () => ({ ok: false, error: { code: "notFound", message: "Node wasn't found" } }),
    });
    const result = await store.start("a.js");
    expect(result.ok).toBe(false);
    expect(store.getState()).toMatchObject({ phase: "idle", error: "Node wasn't found" });
  });

  it("on a stop, shows the top workspace frame and its cheap scopes", async () => {
    const { store, bus, calls, opened } = setup();
    await store.start("m6/shop.js");
    await pause(bus, store);
    const state = store.getState();
    expect(state).toMatchObject({ phase: "paused", threadId: 0, frameId: 8 });
    expect(state.pausedBecause).toBe("Paused on breakpoint");
    expect(opened).toEqual([{ path: "m6/shop.js", line: 4, column: 1 }]);
    expect(state.scopes.map((scope) => scope.variables?.length ?? null)).toEqual([2, null]);
    expect(calls).not.toContain("variables 21");
    expect(store.executionLine("m6/shop.js")).toBe(4);
    expect(store.executionLine("other.js")).toBeNull();

    await store.selectFrame(9);
    expect(store.getState().frameId).toBe(9);
    expect(store.executionLine("m6/shop.js")).toBe(10);
    expect(opened.at(-1)).toEqual({ path: "m6/shop.js", line: 10, column: 3 });
  });

  it("steps, continues and pauses on the stopped thread", async () => {
    const { store, bus, calls } = setup();
    await store.start("a.js");
    await store.pause();
    await pause(bus, store);
    await store.step("over");
    await store.step("out");
    await store.resume();
    expect(calls.filter((call) => /^(resume|pause)/.test(call))).toEqual([
      "resume 0 over",
      "resume 0 out",
      "resume 0",
    ]);
    bus.emit("debug.event", { kind: "continued" });
    expect(store.getState()).toMatchObject({ phase: "running", frames: [], scopes: [] });
    expect(store.executionLine("m6/shop.js")).toBeNull();
  });

  it("evaluates in the selected frame, into the console", async () => {
    const { store, bus, calls } = setup();
    await store.start("a.js");
    await pause(bus, store);
    await store.evaluate("sum + 100");
    await store.evaluate("oops");
    expect(calls).toContain("evaluate sum + 100 8");
    expect(store.getState().output.map((line) => [line.category, line.text])).toEqual([
      ["input", "> sum + 100"],
      ["result", "103"],
      ["input", "> oops"],
      ["stderr", "oops is not defined"],
    ]);
  });

  it("keeps the last output lines only", () => {
    const { store, bus } = setup();
    for (let i = 0; i < MAX_OUTPUT + 5; i++) {
      bus.emit("debug.event", { kind: "output", category: "stdout", text: `line ${i}` });
    }
    const output = store.getState().output;
    expect(output).toHaveLength(MAX_OUTPUT);
    expect(output[0]?.text).toBe("line 5");
  });

  it("ends with the program, and stops a running one when disposed", async () => {
    const { store, bus, calls } = setup();
    await store.start("a.js");
    bus.emit("debug.event", { kind: "terminated" });
    expect(store.getState().phase).toBe("ended");
    expect(calls.at(-1)).toBe("stop");

    const second = setup();
    await second.store.start("a.js");
    second.store.dispose();
    expect(second.calls.at(-1)).toBe("stop");
    // Not listening any more.
    second.bus.emit("debug.event", { kind: "terminated" });
    expect(second.store.getState().phase).toBe("running");
  });
});

function renderView(file: string | undefined = "m6/shop.js", overrides: Partial<DebugPort> = {}) {
  const context = setup(overrides);
  const registry = createCommandRegistry<AppCommandId>();
  const shown: unknown[] = [];
  registry.register({
    id: "view.showView",
    title: "Show View",
    run: (view) => {
      shown.push(view);
    },
  });
  render(
    <DebugView store={context.store} registry={registry} currentFile={() => file} viewId="debug" />,
  );
  return { ...context, registry, shown };
}

describe("DebugView", () => {
  it("runs the file shown, from its button", async () => {
    const { calls, shown } = renderView();
    const button = screen.getByRole("button", { name: "Run and debug shop.js" });
    fireEvent.click(button);
    await screen.findByRole("toolbar", { name: "Debug" });
    expect(shown).toEqual(["debug"]);
    expect(calls[0]).toBe("start m6/shop.js []");
    expect(screen.getByRole("status").textContent).toBe("Running shop.js");
  });

  it("can't run a file that isn't a script", () => {
    const { registry } = renderView("notes.md");
    expect(screen.getByRole("button", { name: "Run and debug" })).toHaveProperty("disabled", true);
    expect(registry.isEnabled("debug.start")).toBe(false);
    expect(registry.isEnabled("debug.stop")).toBe(false);
  });

  it("while paused: the stack, the variables, and F5 continues", async () => {
    const { store, bus, calls, registry } = renderView();
    await act(() => store.start("m6/shop.js"));
    await act(() => pause(bus, store));

    expect(screen.getByRole("status").textContent).toBe("Paused on breakpoint");
    const stack = screen.getByRole("grid", { name: "Call stack" });
    expect(
      within(stack)
        .getAllByRole("row")
        .map((row) => row.textContent),
    ).toEqual(["innerx", "totalshop.js:4", "<anonymous>shop.js:10"]);
    const tree = screen.getByRole("tree", { name: "Variables" });
    expect(within(tree).getByText("sum")).toBeTruthy();
    expect(within(tree).getByText("{name: 'tea'}")).toBeTruthy();

    // An object opens to its properties, read then.
    const item = within(tree).getByRole("treeitem", { name: /^item/ });
    act(() => item.focus());
    fireEvent.keyDown(item, { key: "ArrowRight" });
    await within(tree).findByText("'tea'");
    expect(calls).toContain("variables 30");

    expect(registry.isEnabled("debug.stepOver")).toBe(true);
    expect(registry.isEnabled("debug.pause")).toBe(false);
    await act(() => registry.execute("debug.start"));
    expect(calls.at(-1)).toBe("resume 0");
    fireEvent.click(screen.getByRole("button", { name: "Step over (F10)" }));
    await waitFor(() => expect(calls.at(-1)).toBe("resume 0 over"));
  });

  it("lists breakpoints, and removes one", async () => {
    const { store } = renderView();
    act(() => {
      store.toggleBreakpoint("m6/shop.js", 4);
      store.toggleBreakpoint("m6/shop.js", 11);
    });
    const list = screen.getByRole("grid", { name: "Breakpoints" });
    expect(within(list).getAllByRole("row")).toHaveLength(2);
    fireEvent.click(
      screen.getByRole("button", { name: "Remove the breakpoint at m6/shop.js line 4" }),
    );
    expect(store.getState().breakpoints.get("m6/shop.js")).toEqual([11]);
  });

  it("evaluates what's typed in the console", async () => {
    const { store, bus, calls } = renderView();
    await act(() => store.start("m6/shop.js"));
    await act(() => pause(bus, store));
    const input = screen.getByRole("textbox", { name: "Evaluate an expression" });
    fireEvent.change(input, { target: { value: "sum + 100" } });
    fireEvent.submit(input);
    const log = screen.getByRole("log", { name: "Debug console" });
    await within(log).findByText("103");
    expect(calls).toContain("evaluate sum + 100 8");
    expect((input as HTMLInputElement).value).toBe("");
  });
});
