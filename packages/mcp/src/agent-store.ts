import type {
  AgentCallDto,
  AgentConfigArgs,
  AgentPort,
  AgentStatusDto,
  AppEvents,
  IpcResult,
} from "@repo/contracts";
import type { EventBus } from "@repo/kernel";

/** The most agent calls kept, newest last. */
export const MAX_ACTIVITY = 100;

export interface AgentActivity {
  readonly id: number;
  readonly call: AgentCallDto;
}

export interface AgentState {
  /** `null` until read. */
  readonly status: AgentStatusDto | null;
  /** A change is being applied. */
  readonly busy: boolean;
  /** Why the status couldn't be read, or a change made. */
  readonly failure: string | null;
  readonly activity: readonly AgentActivity[];
}

export interface AgentStore {
  subscribe(listener: () => void): () => void;
  getState(): AgentState;
  /** Reads the status again. */
  refresh(): Promise<void>;
  configure(config: AgentConfigArgs): Promise<void>;
  newToken(): Promise<void>;
  dispose(): void;
}

/** The MCP server's status, and what agents have done, from `agent.call`. */
export function createAgentStore(port: AgentPort, bus: EventBus<AppEvents>): AgentStore {
  let state: AgentState = { status: null, busy: false, failure: null, activity: [] };
  const listeners = new Set<() => void>();
  const set = (next: Partial<AgentState>) => {
    state = { ...state, ...next };
    for (const listener of [...listeners]) listener();
  };
  let callId = 0;
  const stopListening = bus.on("agent.call", (call) => {
    const activity = [...state.activity, { id: ++callId, call }];
    set({ activity: activity.length > MAX_ACTIVITY ? activity.slice(-MAX_ACTIVITY) : activity });
  });

  const apply = async (work: () => Promise<IpcResult<AgentStatusDto>>) => {
    set({ busy: true });
    const result = await work();
    set(
      result.ok
        ? { busy: false, failure: null, status: result.value }
        : { busy: false, failure: result.error.message },
    );
  };

  return {
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    getState: () => state,
    refresh: () => apply(() => port.status()),
    configure: (config) => apply(() => port.configure(config)),
    newToken: () => apply(() => port.newToken()),
    dispose: stopListening,
  };
}

/** The command that adds the server to Claude Code. */
export const claudeCommand = (url: string, token: string): string =>
  `claude mcp add --transport http webide ${url} --header "Authorization: Bearer ${token}"`;

/** The `mcpServers` entry most other clients take. */
export const clientConfig = (url: string, token: string): string =>
  JSON.stringify(
    {
      mcpServers: {
        webide: { type: "http", url, headers: { Authorization: `Bearer ${token}` } },
      },
    },
    null,
    2,
  );
