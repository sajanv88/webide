import type {
  AppEvents,
  AssistantEventDto,
  AssistantPort,
  AssistantStatusDto,
} from "@repo/contracts";
import type { EventBus } from "@repo/kernel";

/** One thing in the conversation, in order. */
export type Entry =
  | { readonly kind: "user"; readonly id: string; readonly text: string }
  | {
      readonly kind: "assistant";
      readonly id: string;
      readonly text: string;
      /** More text is coming. */
      readonly streaming: boolean;
    }
  | {
      readonly kind: "tool";
      readonly id: string;
      readonly name: string;
      /** Its arguments, as JSON. */
      readonly input: string;
      readonly status: "running" | "done" | "error";
      readonly output: string | null;
    }
  | {
      readonly kind: "proposal";
      readonly id: string;
      readonly path: string;
      /** `null` for a new file. */
      readonly original: string | null;
      readonly proposed: string;
      readonly summary: string | null;
      readonly state: "waiting" | "deciding" | "accepted" | "rejected" | "failed";
      readonly error: string | null;
    }
  | { readonly kind: "note"; readonly id: string; readonly text: string; readonly failed: boolean };

export interface AssistantState {
  /** `null` until read. */
  readonly status: AssistantStatusDto | null;
  /** Why the status couldn't be read. */
  readonly failure: string | null;
  readonly conversationId: string;
  readonly entries: readonly Entry[];
  /** From a send until the reply is over. */
  readonly replying: boolean;
}

export interface AssistantStore {
  subscribe(listener: () => void): () => void;
  getState(): AssistantState;
  /** Reads the setup again. */
  refresh(): Promise<void>;
  /** After the setup was saved or forgotten. */
  setStatus(status: AssistantStatusDto): void;
  /** Sends a message, with the file in front of the person. */
  send(text: string, activePath?: string): Promise<void>;
  stop(): Promise<void>;
  decide(callId: string, accept: boolean): Promise<void>;
  /** Forgets this conversation and starts another. */
  newChat(): Promise<void>;
  dispose(): void;
}

const newId = (): string => `chat-${crypto.randomUUID()}`;

/**
 * The assistant's setup and the conversation shown, built from what the
 * backend reports as `assistant.event`.
 */
export function createAssistantStore(
  port: AssistantPort,
  bus: EventBus<AppEvents>,
): AssistantStore {
  let state: AssistantState = {
    status: null,
    failure: null,
    conversationId: newId(),
    entries: [],
    replying: false,
  };
  const listeners = new Set<() => void>();
  const set = (next: Partial<AssistantState>) => {
    state = { ...state, ...next };
    for (const listener of [...listeners]) listener();
  };
  let noteId = 0;
  const note = (text: string, failed: boolean): Entry => ({
    kind: "note",
    id: `note-${++noteId}`,
    text,
    failed,
  });
  const update = (id: string, change: (entry: Entry) => Entry) =>
    state.entries.map((entry) => (entry.id === id ? change(entry) : entry));
  /** The reply's text ends: the last entry, if it's text, stops streaming. */
  const settle = (entries: readonly Entry[]): readonly Entry[] => {
    const last = entries.at(-1);
    return last?.kind === "assistant" && last.streaming
      ? [...entries.slice(0, -1), { ...last, streaming: false }]
      : entries;
  };

  const apply = (event: AssistantEventDto) => {
    if (event.conversationId !== state.conversationId) return;
    switch (event.kind) {
      case "text": {
        const last = state.entries.at(-1);
        if (last?.kind === "assistant" && last.streaming) {
          set({
            entries: [...state.entries.slice(0, -1), { ...last, text: last.text + event.delta }],
          });
        } else {
          set({
            entries: [
              ...state.entries,
              {
                kind: "assistant",
                id: `text-${++noteId}`,
                text: event.delta,
                streaming: true,
              },
            ],
          });
        }
        return;
      }
      case "tool": {
        const status =
          event.status === "running" ? "running" : event.status === "done" ? "done" : "error";
        const entry: Entry = {
          kind: "tool",
          id: event.callId,
          name: event.name,
          input: event.input,
          status,
          output: event.output,
        };
        const known = state.entries.some((each) => each.id === event.callId);
        set({
          entries: known ? update(event.callId, () => entry) : [...settle(state.entries), entry],
        });
        return;
      }
      case "proposal":
        set({
          entries: [
            ...settle(state.entries),
            {
              kind: "proposal",
              id: event.callId,
              path: event.path,
              original: event.original,
              proposed: event.proposed,
              summary: event.summary,
              state: "waiting",
              error: null,
            },
          ],
        });
        return;
      case "proposalDecided":
        set({
          entries: update(event.callId, (entry) =>
            entry.kind === "proposal"
              ? {
                  ...entry,
                  state: event.error ? "failed" : event.accepted ? "accepted" : "rejected",
                  error: event.error,
                }
              : entry,
          ),
        });
        return;
      case "done":
        set({ entries: settle(state.entries), replying: false });
        return;
      case "failed":
        set({
          entries: [...settle(state.entries), note(event.message, event.message !== "Stopped.")],
          replying: false,
        });
        return;
    }
  };
  const stopListening = bus.on("assistant.event", apply);

  return {
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    getState: () => state,
    async refresh() {
      const result = await port.status();
      set(result.ok ? { status: result.value, failure: null } : { failure: result.error.message });
    },
    setStatus: (status) => set({ status, failure: null }),
    async send(text, activePath) {
      if (state.replying || !text.trim()) return;
      set({
        entries: [...state.entries, { kind: "user", id: `user-${++noteId}`, text }],
        replying: true,
      });
      const result = await port.send({
        conversationId: state.conversationId,
        text,
        ...(activePath ? { activePath } : {}),
      });
      if (!result.ok) {
        set({ entries: [...state.entries, note(result.error.message, true)], replying: false });
      }
    },
    async stop() {
      await port.stop(state.conversationId);
    },
    async decide(callId, accept) {
      set({
        entries: update(callId, (entry) =>
          entry.kind === "proposal" ? { ...entry, state: "deciding" } : entry,
        ),
      });
      const result = await port.decide({ conversationId: state.conversationId, callId, accept });
      if (!result.ok) {
        set({
          entries: update(callId, (entry) =>
            entry.kind === "proposal"
              ? { ...entry, state: "failed", error: result.error.message }
              : entry,
          ),
        });
      }
    },
    async newChat() {
      const old = state.conversationId;
      set({ conversationId: newId(), entries: [], replying: false });
      await port.clear(old);
    },
    dispose: stopListening,
  };
}

/** A tool call, in words: "Read src/a.ts". */
export function describeTool(name: string, input: string): string {
  let args: Record<string, unknown> = {};
  try {
    const parsed: unknown = JSON.parse(input);
    if (parsed && typeof parsed === "object") args = parsed as Record<string, unknown>;
  } catch {
    // Arguments that aren't JSON: the name alone.
  }
  const text = (key: string): string | undefined => {
    const value = args[key];
    return typeof value === "string" && value ? value : undefined;
  };
  switch (name) {
    case "workspace_info":
      return "Look at the folder";
    case "list_directory":
      return `List ${text("path") ?? "the folder"}`;
    case "read_file":
      return `Read ${text("path") ?? "a file"}`;
    case "find_files":
      return text("query") ? `Find files like “${text("query")}”` : "List the files";
    case "search_text":
      return `Search for “${text("pattern") ?? ""}”`;
    case "git_status":
      return "Look at git's changes";
    case "git_diff":
      return `Diff ${text("path") ?? "a file"}`;
    case "propose_edit":
      return `Change ${text("path") ?? "a file"}`;
    default:
      return name;
  }
}
