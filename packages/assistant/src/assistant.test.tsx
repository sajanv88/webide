import type {
  AppCommandId,
  AppEvents,
  AssistantEventDto,
  AssistantPort,
  AssistantSetupArgs,
  AssistantStatusDto,
  IpcResult,
  ProviderSetupDto,
} from "@repo/contracts";
import { createCommandRegistry, createEventBus } from "@repo/kernel";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { AssistantPanel } from "./assistant-panel";
import { createAssistantStore, describeTool } from "./assistant-store";
import { SetupDialog } from "./setup-dialog";

const ok = <T,>(value: T): IpcResult<T> => ({ ok: true, value });

const anthropic: ProviderSetupDto = {
  kind: "anthropic",
  model: "claude-sonnet-4-5",
  endpoint: null,
  region: null,
  profile: null,
  bedrockAuth: "profile",
};

/** A backend in memory: it records what it's asked, and keeps the setup. */
function fakePort(start: AssistantStatusDto = { setup: null, hasSecret: false }) {
  let status = start;
  const calls: string[] = [];
  const port: AssistantPort = {
    status: async () => ok(status),
    save: async (args: AssistantSetupArgs) => {
      calls.push(`save ${JSON.stringify(args)}`);
      status = { setup: args.setup, hasSecret: Boolean(args.secret) || status.hasSecret };
      return ok(status);
    },
    forget: async () => {
      status = { setup: null, hasSecret: false };
      return ok(status);
    },
    models: async (args) => {
      calls.push(`models ${args.setup.kind} ${args.secret ?? "-"}`);
      return ok([
        { id: "claude-sonnet-4-5", name: "Claude Sonnet 4.5" },
        { id: "claude-haiku-4-5", name: "Claude Haiku 4.5" },
      ]);
    },
    check: async () => ok("ready"),
    send: async (args) => {
      calls.push(`send ${args.conversationId} ${args.text} ${args.activePath ?? "-"}`);
      return ok(null);
    },
    stop: async (id) => {
      calls.push(`stop ${id}`);
      return ok(null);
    },
    decide: async (args) => {
      calls.push(`decide ${args.callId} ${args.accept}`);
      return ok(null);
    },
    clear: async (id) => {
      calls.push(`clear ${id}`);
      return ok(null);
    },
  };
  return { port, calls };
}

function setup(status?: AssistantStatusDto) {
  const { port, calls } = fakePort(status);
  const bus = createEventBus<AppEvents>();
  const store = createAssistantStore(port, bus);
  const registry = createCommandRegistry<AppCommandId>();
  // The shell's command, which the panel's assistant.focus runs.
  registry.register({ id: "view.showAside", title: "Show", category: "View", run: () => {} });
  const emit = (event: Omit<AssistantEventDto, "conversationId">) =>
    act(() => {
      bus.emit("assistant.event", {
        ...event,
        conversationId: store.getState().conversationId,
      } as AssistantEventDto);
    });
  return { port, calls, bus, store, registry, emit };
}

describe("createAssistantStore", () => {
  it("builds the conversation from what the backend reports", async () => {
    const { store, calls, emit } = setup({ setup: anthropic, hasSecret: true });
    await store.send("Make a 2", "src/a.ts");
    const id = store.getState().conversationId;
    expect(calls).toEqual([`send ${id} Make a 2 src/a.ts`]);
    expect(store.getState().replying).toBe(true);

    emit({ kind: "text", delta: "Let me " });
    emit({ kind: "text", delta: "look." });
    emit({
      kind: "tool",
      callId: "c1",
      name: "read_file",
      input: '{"path":"a.ts"}',
      status: "running",
      output: null,
    });
    emit({
      kind: "tool",
      callId: "c1",
      name: "read_file",
      input: '{"path":"a.ts"}',
      status: "done",
      output: "let a = 1;",
    });
    emit({
      kind: "proposal",
      callId: "c2",
      path: "a.ts",
      original: "let a = 1;",
      proposed: "let a = 2;",
      summary: "Bump a.",
    });
    emit({ kind: "proposalDecided", callId: "c2", accepted: true, error: null });
    emit({ kind: "text", delta: "Done." });
    emit({ kind: "done" });

    const entries = store.getState().entries;
    expect(entries.map((entry) => entry.kind)).toEqual([
      "user",
      "assistant",
      "tool",
      "proposal",
      "assistant",
    ]);
    expect(entries[1]).toMatchObject({ text: "Let me look.", streaming: false });
    expect(entries[2]).toMatchObject({ status: "done", output: "let a = 1;" });
    expect(entries[3]).toMatchObject({ state: "accepted" });
    expect(store.getState().replying).toBe(false);
  });

  it("ignores other conversations, and says why a reply stopped", async () => {
    const { store, bus, emit } = setup();
    await store.send("Hi");
    act(() => {
      bus.emit("assistant.event", { kind: "text", conversationId: "chat-other", delta: "x" });
    });
    expect(store.getState().entries).toHaveLength(1);
    emit({ kind: "failed", message: "Anthropic refused the request (HTTP 401): bad key" });
    expect(store.getState().entries.at(-1)).toMatchObject({ kind: "note", failed: true });
    expect(store.getState().replying).toBe(false);
  });

  it("starts a new chat and forgets the old one", async () => {
    const { store, calls } = setup();
    await store.send("Hi");
    const old = store.getState().conversationId;
    await store.newChat();
    expect(store.getState().conversationId).not.toBe(old);
    expect(store.getState().entries).toEqual([]);
    expect(calls.at(-1)).toBe(`clear ${old}`);
  });
});

describe("describeTool", () => {
  it("says what a tool did in words", () => {
    expect(describeTool("read_file", '{"path":"src/a.ts"}')).toBe("Read src/a.ts");
    expect(describeTool("search_text", '{"pattern":"TODO"}')).toBe("Search for “TODO”");
    expect(describeTool("list_directory", "{}")).toBe("List the folder");
    expect(describeTool("mystery", "not json")).toBe("mystery");
  });
});

describe("SetupDialog", () => {
  it("guides through provider, key, model and check, then saves", async () => {
    const { port, calls, store, registry } = setup();
    await store.refresh();
    render(<SetupDialog store={store} port={port} registry={registry} />);
    await act(async () => {
      await registry.execute("assistant.setup");
    });
    const dialog = await screen.findByRole("dialog", { name: "Set up the AI assistant" });
    expect(within(dialog).getByRole("list", { name: "Setting up the assistant" })).toBeTruthy();

    // 1. The provider.
    fireEvent.click(within(dialog).getByRole("radio", { name: /Anthropic/ }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Next" }));

    // 2. The key is needed.
    await within(dialog).findByRole("heading", { name: "Connect to Anthropic" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Next" }));
    expect(await within(dialog).findByText("Enter the API key.")).toBeTruthy();
    fireEvent.change(within(dialog).getByLabelText("API key"), {
      target: { value: "sk-test" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Next" }));

    // 3. The models are listed with the key typed; one is typed in.
    await within(dialog).findByRole("heading", { name: "Which model?" });
    await waitFor(() => expect(calls).toContain("models anthropic sk-test"));
    fireEvent.change(within(dialog).getByLabelText("Model"), {
      target: { value: "claude-haiku-4-5" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Next" }));

    // 4. The check, then the save.
    await within(dialog).findByRole("heading", { name: "Check that it answers" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Check the connection" }));
    expect(await within(dialog).findByText("The model said: “ready”")).toBeTruthy();
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    const saved = calls.find((call) => call.startsWith("save "));
    expect(JSON.parse(saved?.slice(5) ?? "{}")).toEqual({
      setup: { ...anthropic, model: "claude-haiku-4-5" },
      secret: "sk-test",
    });
    expect(store.getState().status?.setup?.model).toBe("claude-haiku-4-5");
  });

  it("asks Bedrock for a region and how to sign in, not for a key with a profile", async () => {
    const { port, store, registry } = setup();
    await store.refresh();
    render(<SetupDialog store={store} port={port} registry={registry} />);
    await act(async () => {
      await registry.execute("assistant.setup");
    });
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("radio", { name: /Amazon Bedrock/ }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Next" }));
    await within(dialog).findByRole("heading", { name: "Connect to Amazon Bedrock" });
    expect(within(dialog).getByLabelText("Region")).toBeTruthy();
    expect(within(dialog).getByLabelText("Profile")).toBeTruthy();
    expect(within(dialog).queryByLabelText("API key")).toBeNull();
    fireEvent.click(within(dialog).getByRole("radio", { name: "A Bedrock API key" }));
    expect(within(dialog).getByLabelText("API key")).toBeTruthy();
  });
});

describe("AssistantPanel", () => {
  const renderDiff = (path: string, original: string, proposed: string) => (
    <pre data-testid="diff">{`${path}: ${original} -> ${proposed}`}</pre>
  );

  it("offers the setup until there's one", async () => {
    const { store, registry } = setup();
    await store.refresh();
    let asked = false;
    registry.register({
      id: "assistant.setup",
      title: "Set up",
      category: "Assistant",
      run: () => {
        asked = true;
      },
    });
    render(<AssistantPanel store={store} registry={registry} renderDiff={renderDiff} />);
    fireEvent.click(await screen.findByRole("button", { name: "Set up" }));
    expect(asked).toBe(true);
  });

  it("shows the reply, the tools, and a change to accept", async () => {
    const { store, registry, calls, emit } = setup({ setup: anthropic, hasSecret: true });
    await store.refresh();
    render(
      <AssistantPanel
        store={store}
        registry={registry}
        renderDiff={renderDiff}
        activePath={() => "src/a.ts"}
      />,
    );
    expect(screen.getByText("Anthropic · claude-sonnet-4-5")).toBeTruthy();
    const box = screen.getByRole("textbox", { name: "Message the assistant" });
    fireEvent.change(box, { target: { value: "Make a 2" } });
    fireEvent.submit(box.closest("form") as HTMLFormElement);
    await waitFor(() => expect(calls.at(-1)).toMatch(/^send chat-.+ Make a 2 src\/a\.ts$/));

    emit({ kind: "text", delta: "Reading **a.ts** first." });
    emit({
      kind: "tool",
      callId: "c1",
      name: "read_file",
      input: '{"path":"a.ts"}',
      status: "done",
      output: "let a = 1;",
    });
    emit({
      kind: "proposal",
      callId: "c2",
      path: "a.ts",
      original: "let a = 1;",
      proposed: "let a = 2;",
      summary: "Bump a.",
    });
    expect(screen.getByText("a.ts", { selector: "strong" })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Read a\.ts/ })).toBeTruthy();
    expect(screen.getByTestId("diff").textContent).toBe("a.ts: let a = 1; -> let a = 2;");
    expect(screen.getByText("Write this change to a.ts?")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Accept" }));
    await waitFor(() => expect(calls).toContain("decide c2 true"));
    emit({ kind: "proposalDecided", callId: "c2", accepted: true, error: null });
    expect(screen.getByRole("button", { name: /Change a\.ts.*Accepted/ })).toBeTruthy();
  });

  it("doesn't follow a link in a reply", async () => {
    const { store, registry, emit } = setup({ setup: anthropic, hasSecret: true });
    await store.refresh();
    render(<AssistantPanel store={store} registry={registry} renderDiff={renderDiff} />);
    await store.send("Docs?");
    emit({ kind: "text", delta: "See [the docs](https://example.com/docs)." });
    emit({ kind: "done" });
    expect(document.querySelector("a[href]")).toBeNull();
    expect(screen.getByText(/the docs/).textContent).toContain("https://example.com/docs");
  });
});
