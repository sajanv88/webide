import type {
  AgentConfigArgs,
  AgentPort,
  AgentStatusDto,
  AppCommandId,
  AppEvents,
  IpcResult,
} from "@repo/contracts";
import { createCommandRegistry, createEventBus } from "@repo/kernel";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { AgentStatusItem } from "./agent-status";
import { claudeCommand, clientConfig, createAgentStore, MAX_ACTIVITY } from "./agent-store";
import { AgentsDialog } from "./agents-dialog";

const TOKEN = "a".repeat(64);

/** A server in memory: running while enabled, except on port 1999. */
function fakePort() {
  let config: AgentConfigArgs = { enabled: false, allowEdits: false, port: 7341 };
  let token = TOKEN;
  const calls: string[] = [];
  const status = (): IpcResult<AgentStatusDto> => ({
    ok: true,
    value: {
      ...config,
      url: config.enabled && config.port !== 1999 ? `http://127.0.0.1:${config.port}/mcp` : null,
      token,
      error: config.enabled && config.port === 1999 ? "port 1999 is in use" : null,
    },
  });
  const port: AgentPort = {
    status: async () => status(),
    configure: async (next) => {
      calls.push(`configure ${JSON.stringify(next)}`);
      config = next;
      return status();
    },
    newToken: async () => {
      calls.push("newToken");
      token = "b".repeat(64);
      return status();
    },
  };
  return { port, calls };
}

function setup() {
  const { port, calls } = fakePort();
  const bus = createEventBus<AppEvents>();
  const store = createAgentStore(port, bus);
  return { store, bus, calls, port };
}

describe("createAgentStore", () => {
  it("reads and changes the status", async () => {
    const { store, calls } = setup();
    expect(store.getState().status).toBeNull();
    await store.refresh();
    expect(store.getState().status?.enabled).toBe(false);
    await store.configure({ enabled: true, allowEdits: false, port: 7341 });
    expect(store.getState().status?.url).toBe("http://127.0.0.1:7341/mcp");
    await store.newToken();
    expect(store.getState().status?.token).toBe("b".repeat(64));
    expect(calls).toEqual([
      'configure {"enabled":true,"allowEdits":false,"port":7341}',
      "newToken",
    ]);
  });

  it("keeps the latest agent calls, and says why a change failed", async () => {
    const { store, bus } = setup();
    for (let i = 0; i < MAX_ACTIVITY + 3; i++) {
      bus.emit("agent.call", { tool: "read_file", target: `f${i}`, error: null });
    }
    const activity = store.getState().activity;
    expect(activity).toHaveLength(MAX_ACTIVITY);
    expect(activity[0]?.call.target).toBe("f3");

    const failing = createAgentStore(
      {
        status: async () => ({ ok: false, error: { code: "io", message: "disk full" } }),
        configure: async () => ({ ok: false, error: { code: "io", message: "disk full" } }),
        newToken: async () => ({ ok: false, error: { code: "io", message: "disk full" } }),
      },
      bus,
    );
    await failing.refresh();
    expect(failing.getState()).toMatchObject({ failure: "disk full", busy: false });
    store.dispose();
    bus.emit("agent.call", { tool: "read_file", target: "after", error: null });
    expect(store.getState().activity.at(-1)?.call.target).not.toBe("after");
  });

  it("writes the connection for Claude Code and for other clients", () => {
    const url = "http://127.0.0.1:7341/mcp";
    expect(claudeCommand(url, "t")).toBe(
      'claude mcp add --transport http webide http://127.0.0.1:7341/mcp --header "Authorization: Bearer t"',
    );
    expect(JSON.parse(clientConfig(url, "t"))).toEqual({
      mcpServers: { webide: { type: "http", url, headers: { Authorization: "Bearer t" } } },
    });
  });
});

function renderDialog() {
  const context = setup();
  const registry = createCommandRegistry<AppCommandId>();
  const copy = vi.fn(async (_text: string) => {});
  render(
    <>
      <AgentsDialog store={context.store} registry={registry} copy={copy} />
      <AgentStatusItem store={context.store} registry={registry} />
    </>,
  );
  return { ...context, registry, copy };
}

describe("AgentsDialog", () => {
  it("turns the server on, shows how to connect, and copies the command", async () => {
    const { registry, calls, copy } = renderDialog();
    await act(() => registry.execute("agents.open"));
    const dialog = await screen.findByRole("dialog", { name: "AI Agents" });
    const enabled = within(dialog).getByRole("switch", { name: "Let agents use the open folder" });
    expect(enabled.getAttribute("aria-checked")).toBe("false");
    expect(within(dialog).getByRole("status").textContent).toBe("Off.");
    expect(within(dialog).queryByRole("region", { name: "Connecting an agent" })).toBeNull();

    fireEvent.click(enabled);
    await within(dialog).findByRole("region", { name: "Connecting an agent" });
    expect(calls).toEqual(['configure {"enabled":true,"allowEdits":false,"port":7341}']);
    expect(within(dialog).getByRole("status").textContent).toBe(
      "Running at http://127.0.0.1:7341/mcp, read only.",
    );
    // The token is hidden until asked for.
    const command = within(dialog).getByRole("textbox", {
      name: "Claude Code",
    }) as HTMLInputElement;
    expect(command.value).not.toContain(TOKEN);
    fireEvent.click(within(dialog).getByRole("button", { name: "Copy the command" }));
    await waitFor(() =>
      expect(copy).toHaveBeenCalledWith(claudeCommand("http://127.0.0.1:7341/mcp", TOKEN)),
    );
    await within(dialog).findByRole("button", { name: "Copy the command: copied" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Show the token" }));
    expect(command.value).toContain(TOKEN);

    // The status bar says agents are on, and opens this.
    expect(
      screen.getByRole("button", { name: /^AI agents can read this folder/, hidden: true }),
    ).toBeTruthy();
  });

  it("allows edits, changes the port, and makes a new token", async () => {
    const { registry, calls } = renderDialog();
    await act(() => registry.execute("agents.open"));
    const dialog = await screen.findByRole("dialog", { name: "AI Agents" });
    fireEvent.click(within(dialog).getByRole("switch", { name: "Let agents use the open folder" }));
    await within(dialog).findByRole("region", { name: "Connecting an agent" });
    fireEvent.click(within(dialog).getByRole("switch", { name: "Allow agents to edit files" }));
    await waitFor(() =>
      expect(within(dialog).getByRole("status").textContent).toContain("edits allowed"),
    );

    const port = within(dialog).getByRole("spinbutton", { name: "Port" });
    fireEvent.change(port, { target: { value: "1999" } });
    fireEvent.blur(port);
    fireEvent.click(within(dialog).getByRole("button", { name: "Use this port" }));
    await within(dialog).findByText("port 1999 is in use");
    expect(screen.getByRole("button", { name: /server isn.t running/, hidden: true })).toBeTruthy();

    fireEvent.change(port, { target: { value: "7400" } });
    fireEvent.blur(port);
    fireEvent.click(within(dialog).getByRole("button", { name: "Use this port" }));
    await within(dialog).findByRole("region", { name: "Connecting an agent" });
    fireEvent.click(within(dialog).getByRole("button", { name: "New token" }));
    await waitFor(() => expect(calls.at(-1)).toBe("newToken"));
    expect(calls).toEqual([
      'configure {"enabled":true,"allowEdits":false,"port":7341}',
      'configure {"enabled":true,"allowEdits":true,"port":7341}',
      'configure {"enabled":true,"allowEdits":true,"port":1999}',
      'configure {"enabled":true,"allowEdits":true,"port":7400}',
      "newToken",
    ]);
  });

  it("lists what agents did", async () => {
    const { registry, bus } = renderDialog();
    await act(() => registry.execute("agents.open"));
    const dialog = await screen.findByRole("dialog", { name: "AI Agents" });
    expect(within(dialog).getByText("Nothing yet.")).toBeTruthy();
    act(() => {
      bus.emit("agent.call", { tool: "read_file", target: "src/a.ts", error: null });
      bus.emit("agent.call", { tool: "write_file", target: "b.ts", error: "edits aren't allowed" });
    });
    const list = within(dialog).getByRole("list", { name: "What agents did" });
    expect(
      within(list)
        .getAllByRole("listitem")
        .map((item) => item.textContent),
    ).toEqual(["read_filesrc/a.ts", "write_fileb.tsfailed: edits aren't allowed"]);
  });
});
