import { describe, expect, it, vi } from "vitest";
import { createCommandRegistry, createEventBus, type ExecuteResult } from "./index";

type Id = "file.open" | "file.save" | "view.toggle";

describe("command registry", () => {
  it("keeps a command's category and palette flag for the menus and the palette", () => {
    const registry = createCommandRegistry<Id>();
    registry.register({ id: "file.save", title: "Save", category: "File", run: () => {} });
    registry.register({ id: "file.open", title: "Open", palette: false, run: () => {} });
    expect(registry.get("file.save")?.category).toBe("File");
    expect(
      registry
        .all()
        .filter((command) => command.palette !== false)
        .map((c) => c.id),
    ).toEqual(["file.save"]);
  });

  it("registers, lists and disposes commands", () => {
    const registry = createCommandRegistry<Id>();
    const dispose = registry.register({ id: "file.open", title: "Open", run: () => {} });

    expect(registry.get("file.open")?.title).toBe("Open");
    expect(registry.all().map((c) => c.id)).toEqual(["file.open"]);

    dispose();
    expect(registry.get("file.open")).toBeUndefined();
    dispose(); // a second call does nothing
    expect(registry.all()).toEqual([]);
  });

  it("refuses a duplicate id", () => {
    const registry = createCommandRegistry<Id>();
    registry.register({ id: "file.open", title: "Open", run: () => {} });
    expect(() => registry.register({ id: "file.open", title: "Again", run: () => {} })).toThrow(
      /already registered/,
    );
  });

  it("a stale dispose doesn't remove a newer command with the same id", () => {
    const registry = createCommandRegistry<Id>();
    const first = registry.register({ id: "file.open", title: "First", run: () => {} });
    first();
    registry.register({ id: "file.open", title: "Second", run: () => {} });
    first();
    expect(registry.get("file.open")?.title).toBe("Second");
  });

  const executeCases: ReadonlyArray<readonly [string, (() => boolean) | undefined, ExecuteResult]> =
    [
      ["an unknown command", undefined, { ok: false, reason: "notRegistered" }],
      ["a disabled command", () => false, { ok: false, reason: "disabled" }],
      ["an enabled command", () => true, { ok: true }],
    ];

  it.each(executeCases)("executing %s", async (_label, isEnabled, expected) => {
    const registry = createCommandRegistry<Id>();
    const run = vi.fn();
    if (isEnabled) registry.register({ id: "file.save", title: "Save", isEnabled, run });
    expect(await registry.execute("file.save", { path: "a.ts" })).toEqual(expected);
    expect(run).toHaveBeenCalledTimes(expected.ok ? 1 : 0);
    if (expected.ok) expect(run).toHaveBeenCalledWith({ path: "a.ts" });
  });

  it("turns a failing command into a result and reports it", async () => {
    const onError = vi.fn();
    const registry = createCommandRegistry<Id>({ onError });
    const boom = new Error("boom");
    registry.register({
      id: "file.save",
      title: "Save",
      run: async () => {
        throw boom;
      },
    });

    expect(await registry.execute("file.save")).toEqual({
      ok: false,
      reason: "failed",
      error: boom,
    });
    expect(onError).toHaveBeenCalledWith("file.save", boom);
  });

  it("notifies subscribers and bumps the version on every change", () => {
    const registry = createCommandRegistry<Id>();
    const listener = vi.fn();
    const unsubscribe = registry.subscribe(listener);
    const start = registry.version();

    const dispose = registry.register({ id: "view.toggle", title: "Toggle", run: () => {} });
    registry.notify();
    dispose();

    expect(listener).toHaveBeenCalledTimes(3);
    expect(registry.version()).toBe(start + 3);
    unsubscribe();
    registry.notify();
    expect(listener).toHaveBeenCalledTimes(3);
  });

  it("asks isEnabled each time", () => {
    const registry = createCommandRegistry<Id>();
    let open = false;
    registry.register({ id: "file.save", title: "Save", isEnabled: () => open, run: () => {} });
    expect(registry.isEnabled("file.save")).toBe(false);
    open = true;
    expect(registry.isEnabled("file.save")).toBe(true);
    expect(registry.isEnabled("file.open")).toBe(false);
  });
});

describe("event bus", () => {
  interface Events {
    opened: { path: string };
    closed: undefined;
  }

  it("delivers payloads to the listeners of that event only", () => {
    const bus = createEventBus<Events>();
    const opened = vi.fn();
    const closed = vi.fn();
    bus.on("opened", opened);
    bus.on("closed", closed);

    bus.emit("opened", { path: "a.ts" });

    expect(opened).toHaveBeenCalledWith({ path: "a.ts" });
    expect(closed).not.toHaveBeenCalled();
  });

  it("unsubscribes, even from inside a listener", () => {
    const bus = createEventBus<Events>();
    const second = vi.fn();
    const off = bus.on("opened", () => off());
    bus.on("opened", second);

    bus.emit("opened", { path: "a" });
    bus.emit("opened", { path: "b" });

    expect(second).toHaveBeenCalledTimes(2);
  });

  it("keeps going when a listener throws", () => {
    const onListenerError = vi.fn();
    const bus = createEventBus<Events>({ onListenerError });
    const after = vi.fn();
    bus.on("opened", () => {
      throw new Error("bad listener");
    });
    bus.on("opened", after);

    bus.emit("opened", { path: "a" });

    expect(after).toHaveBeenCalled();
    expect(onListenerError).toHaveBeenCalledWith(expect.any(Error), "opened");
  });
});
