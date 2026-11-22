import type { BackendEventDto, DirListingDto } from "@repo/contracts";
import type { Channel } from "@tauri-apps/api/core";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it } from "vitest";
import {
  appInfo,
  call,
  closeWindow,
  listDirectory,
  setCloseGuard,
  subscribeEvents,
  toIpcError,
  updateSettings,
} from "./index";

afterEach(() => {
  clearMocks();
});

describe("call", () => {
  it("sends input under `args` and resolves the output", async () => {
    const listing: DirListingDto = { path: "src", entries: [], truncated: false, skipped: 0 };
    const calls: Array<[string, unknown]> = [];
    mockIPC((command, payload) => {
      calls.push([command, payload]);
      return listing;
    });

    const result = await listDirectory({ workspaceId: "1", path: "src" });

    expect(result).toEqual({ ok: true, value: listing });
    expect(calls).toEqual([["list_directory", { args: { workspaceId: "1", path: "src" } }]]);
  });

  it("sends no args for commands that take none", async () => {
    const calls: Array<[string, unknown]> = [];
    mockIPC((command, payload) => {
      calls.push([command, payload]);
      return { name: "webide", version: "0.1.0" };
    });

    expect(await appInfo()).toEqual({ ok: true, value: { name: "webide", version: "0.1.0" } });
    expect(calls).toHaveLength(1);
    expect(calls[0]?.[0]).toBe("app_info");
    expect(calls[0]?.[1]).not.toHaveProperty("args");
  });

  it("returns a command's IpcError as a value", async () => {
    mockIPC(() => {
      throw { code: "outsideWorkspace", message: "the path is outside the workspace" };
    });

    const result = await call("read_file", { workspaceId: "1", path: "x" });

    expect(result).toEqual({
      ok: false,
      error: { code: "outsideWorkspace", message: "the path is outside the workspace" },
    });
  });

  it("passes partial settings patches through", async () => {
    const calls: unknown[] = [];
    mockIPC((_command, payload) => {
      calls.push(payload);
      return { theme: "dark", readOnlyAboveBytes: 1, refuseAboveBytes: 2 };
    });
    await updateSettings({ theme: "dark" });
    expect(calls).toEqual([{ args: { theme: "dark" } }]);
  });
});

describe("toIpcError", () => {
  it.each([
    [
      "a plain string",
      "command list_directory not allowed by ACL",
      "command list_directory not allowed by ACL",
    ],
    ["an Error", new Error("boom"), "boom"],
    ["an unknown code", { code: "madeUp", message: "x" }, "[object Object]"],
    ["a code without a message", { code: "notFound" }, "[object Object]"],
    ["null", null, "null"],
  ])("turns %s into an internal error", (_label, rejection, message) => {
    expect(toIpcError(rejection)).toEqual({ code: "internal", message });
  });

  it("keeps a well-formed IpcError", () => {
    expect(toIpcError({ code: "notFound", message: "not found" })).toEqual({
      code: "notFound",
      message: "not found",
    });
  });
});

describe("backend events", () => {
  it("passes a channel to subscribe_events and hands its messages on until stopped", async () => {
    let channel: Channel<BackendEventDto> | undefined;
    mockIPC((command, payload) => {
      expect(command).toBe("subscribe_events");
      channel = (payload as { args: { channel: Channel<BackendEventDto> } }).args.channel;
      return null;
    });
    const heard: BackendEventDto[] = [];
    const result = await subscribeEvents((event) => heard.push(event));
    if (!result.ok || !channel) throw new Error("not subscribed");

    channel.onmessage({ kind: "closeRequested" });
    expect(heard).toEqual([{ kind: "closeRequested" }]);

    result.value();
    channel.onmessage({ kind: "closeRequested" });
    expect(heard).toHaveLength(1);
  });

  it("sends the close guard, and closes the window", async () => {
    const calls: Array<[string, unknown]> = [];
    mockIPC((command, payload) => {
      calls.push([command, payload]);
      return null;
    });
    await setCloseGuard(true);
    await closeWindow();
    expect(calls[0]).toEqual(["set_close_guard", { args: { unsaved: true } }]);
    expect(calls[1]?.[0]).toBe("close_window");
  });
});
