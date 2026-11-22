import type {
  AppCommandId,
  AppEvents,
  IpcResult,
  OpenTerminal,
  TerminalEventDto,
  TerminalPort,
} from "@repo/contracts";
import { createCommandRegistry, createEventBus } from "@repo/kernel";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { allowTerminalStyles } from "./csp";
import { TerminalPanel } from "./terminal-panel";
import { fit } from "./terminal-view";
import { luminance } from "./theme";

const ok = <T,>(value: T): IpcResult<T> => ({ ok: true, value });

function fakePort() {
  const calls: string[] = [];
  const listeners = new Map<number, (event: TerminalEventDto) => void>();
  let next = 1;
  const port: TerminalPort = {
    open: async (cols, rows, onEvent): Promise<IpcResult<OpenTerminal>> => {
      const id = next++;
      calls.push(`open ${id} ${cols}x${rows}`);
      listeners.set(id, onEvent);
      return ok({ terminal: { id, shell: "fakesh" }, stop: () => calls.push(`stop ${id}`) });
    },
    write: async (id, data) => {
      calls.push(`write ${id} ${data}`);
      return ok(null);
    },
    resize: async (id, cols, rows) => {
      calls.push(`resize ${id} ${cols}x${rows}`);
      return ok(null);
    },
    close: async (id) => {
      calls.push(`close ${id}`);
      return ok(null);
    },
  };
  const send = (id: number, event: TerminalEventDto) => listeners.get(id)?.(event);
  return { port, calls, send };
}

function renderPanel(port: TerminalPort) {
  const registry = createCommandRegistry<AppCommandId>();
  const bus = createEventBus<AppEvents>();
  const shown = vi.fn();
  registry.register({ id: "view.showPanel", title: "Show Panel", palette: false, run: shown });
  render(<TerminalPanel port={port} registry={registry} bus={bus} />);
  return { registry, shown };
}

describe("fit", () => {
  it("fits whole cells, leaving room for the scrollbar", () => {
    expect(fit({ width: 814, height: 240 }, { width: 10, height: 20 })).toEqual({
      cols: 80,
      rows: 12,
    });
    expect(fit({ width: 0, height: 0 }, { width: 10, height: 20 })).toBeNull();
    expect(fit({ width: 500, height: 500 }, { width: 0, height: 0 })).toBeNull();
  });
});

describe("theme", () => {
  it("knows a dark background from a light one", () => {
    expect(luminance("#000000")).toBe(0);
    expect(luminance("#ffffff")).toBeCloseTo(1);
    expect(luminance("#111827")).toBeLessThan(0.4);
  });
});

describe("allowTerminalStyles", () => {
  it("gives runtime style elements the nonce, and inline styles go through the style object", () => {
    allowTerminalStyles("n0nce");
    const style = document.createElement("style");
    expect(style.getAttribute("nonce")).toBe("n0nce");
    expect(document.createElement("div").getAttribute("nonce")).toBeNull();
    const span = document.createElement("span");
    span.setAttribute("style", "color: rgb(255, 0, 0)");
    expect(span.style.color).toBe("rgb(255, 0, 0)");
    span.setAttribute("title", "plain attributes are unchanged");
    expect(span.getAttribute("title")).toBe("plain attributes are unchanged");
  });
});

describe("TerminalPanel", () => {
  it("starts with nothing open, and starts a shell when asked", async () => {
    const { port, calls } = fakePort();
    renderPanel(port);
    expect(screen.getByText("No terminal open")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "New terminal" }));
    const tabs = await screen.findByRole("tablist", { name: "Terminals" });
    await waitFor(() => expect(within(tabs).getByRole("tab").textContent).toBe("1: fakesh"));
    expect(calls[0]).toMatch(/^open 1 \d+x\d+$/);
  });

  it("marks a terminal whose shell exited, and kills the one shown", async () => {
    const { port, calls, send } = fakePort();
    renderPanel(port);
    fireEvent.click(screen.getByRole("button", { name: "New terminal" }));
    await screen.findByRole("tab", { name: "1: fakesh" });
    fireEvent.click(screen.getByRole("button", { name: "New terminal" }));
    await screen.findByRole("tab", { name: "2: fakesh" });

    act(() => send(2, { kind: "exited", code: 0 }));
    expect(await screen.findByRole("tab", { name: "2: fakesh (exited)" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Kill terminal" }));
    await waitFor(() => expect(calls).toContain("close 2"));
    expect(screen.queryByRole("tab", { name: /^2:/ })).toBeNull();
    expect(screen.getByRole("tab", { name: "1: fakesh" }).getAttribute("aria-selected")).toBe(
      "true",
    );
  });

  it("registers its commands; Toggle Terminal shows the panel and starts one", async () => {
    const { port, calls } = fakePort();
    const { registry, shown } = renderPanel(port);
    for (const id of ["terminal.new", "terminal.kill", "terminal.focus", "terminal.screenReader"]) {
      expect(registry.get(id as AppCommandId), id).toBeTruthy();
    }
    expect(registry.isEnabled("terminal.kill")).toBe(false);
    await act(() => registry.execute("terminal.focus"));
    expect(shown).toHaveBeenCalled();
    await waitFor(() => expect(calls.some((call) => call.startsWith("open 1"))).toBe(true));
    await waitFor(() => expect(registry.isEnabled("terminal.kill")).toBe(true));
  });
});
