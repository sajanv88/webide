import type {
  AppEvents,
  DebugPort,
  IpcResult,
  ScopeDto,
  StackFrameDto,
  VariableDto,
} from "@repo/contracts";
import type { EventBus } from "@repo/kernel";

/** The most output lines kept. */
export const MAX_OUTPUT = 2000;

export type Phase = "idle" | "starting" | "running" | "paused" | "ended";

export interface OutputLine {
  readonly id: number;
  /** `stdout`, `stderr`, `console` (the debugger's), `input` (an evaluation). */
  readonly category: string;
  readonly text: string;
}

export interface PausedScope {
  readonly scope: ScopeDto;
  /** Read when the scope is opened; the cheap ones at once. */
  readonly variables: readonly VariableDto[] | null;
}

export interface DebugState {
  readonly phase: Phase;
  /** The file being run. */
  readonly program: string | null;
  readonly threadId: number | null;
  /** Why it paused, in words: "Paused on breakpoint". */
  readonly pausedBecause: string | null;
  readonly frames: readonly StackFrameDto[];
  readonly frameId: number | null;
  readonly scopes: readonly PausedScope[];
  readonly output: readonly OutputLine[];
  /** Breakpoint lines (1-based) by workspace path. */
  readonly breakpoints: ReadonlyMap<string, readonly number[]>;
  readonly error: string | null;
}

export interface DebugStore {
  subscribe(listener: () => void): () => void;
  getState(): DebugState;
  start(path: string): Promise<IpcResult<null>>;
  stop(): Promise<void>;
  resume(): Promise<void>;
  step(how: "over" | "into" | "out"): Promise<void>;
  pause(): Promise<void>;
  toggleBreakpoint(path: string, line: number): void;
  removeBreakpoint(path: string, line: number): void;
  selectFrame(frameId: number): Promise<void>;
  /** An object's properties (or a scope's variables), by its reference. */
  children(reference: number): Promise<readonly VariableDto[]>;
  evaluate(expression: string): Promise<void>;
  /** The paused line in `path`, for its editor. */
  executionLine(path: string): number | null;
  dispose(): void;
}

const SCRIPT = /\.(?:[mc]?[jt]s)$/i;

/** Whether a file can be run under the debugger. */
export const debuggable = (path: string): boolean => SCRIPT.test(path);

const sentence = (reason: string, description: string | null | undefined) =>
  description ?? `Paused on ${reason}`;

/** The debugger's state for one workspace, and what can be done with it. */
export function createDebugStore(port: DebugPort, bus: EventBus<AppEvents>): DebugStore {
  let state: DebugState = {
    phase: "idle",
    program: null,
    threadId: null,
    pausedBecause: null,
    frames: [],
    frameId: null,
    scopes: [],
    output: [],
    breakpoints: new Map(),
    error: null,
  };
  const listeners = new Set<() => void>();
  const set = (next: Partial<DebugState>) => {
    state = { ...state, ...next };
    for (const listener of [...listeners]) listener();
  };
  let lineId = 0;
  const write = (category: string, text: string) => {
    const lines = [...state.output, { id: ++lineId, category, text }];
    set({ output: lines.length > MAX_OUTPUT ? lines.slice(-MAX_OUTPUT) : lines });
  };

  const loadScopes = async (frameId: number) => {
    const result = await port.scopes(frameId);
    if (!result.ok) return;
    const scopes = await Promise.all(
      result.value.map(async (scope): Promise<PausedScope> => {
        if (scope.expensive || scope.reference === 0) return { scope, variables: null };
        const variables = await port.variables(scope.reference);
        return { scope, variables: variables.ok ? variables.value : [] };
      }),
    );
    if (state.frameId === frameId) set({ scopes });
  };

  const show = (frame: StackFrameDto) => {
    if (frame.source?.kind === "workspace") {
      bus.emit("file.openRequested", {
        path: frame.source.path,
        line: frame.line,
        column: Math.max(1, frame.column),
      });
    }
  };

  const onStopped = async (threadId: number, reason: string, description: string | null) => {
    set({
      phase: "paused",
      threadId,
      pausedBecause: sentence(reason, description),
      frames: [],
      scopes: [],
    });
    const stack = await port.stack(threadId);
    if (!stack.ok || state.threadId !== threadId) return;
    const top = stack.value.find((frame) => frame.source?.kind === "workspace") ?? stack.value[0];
    set({ frames: stack.value, frameId: top?.id ?? null });
    if (top) {
      show(top);
      await loadScopes(top.id);
    }
  };

  const stopListening = bus.on("debug.event", (event) => {
    switch (event.kind) {
      case "output":
        write(event.category, event.text);
        break;
      case "stopped":
        void onStopped(event.threadId, event.reason, event.description);
        break;
      case "continued":
        set({ phase: "running", pausedBecause: null, frames: [], frameId: null, scopes: [] });
        break;
      case "terminated":
        set({
          phase: "ended",
          threadId: null,
          pausedBecause: null,
          frames: [],
          frameId: null,
          scopes: [],
        });
        // The backend's session ends with the program.
        void port.stop();
        break;
    }
  });

  const list = () => [...state.breakpoints].map(([path, lines]) => ({ path, lines: [...lines] }));

  const setLines = (path: string, lines: readonly number[]) => {
    const next = new Map(state.breakpoints);
    if (lines.length === 0) next.delete(path);
    else
      next.set(
        path,
        [...lines].sort((a, b) => a - b),
      );
    set({ breakpoints: next });
    // Running: the debugger takes them now.
    if (state.phase === "running" || state.phase === "paused") {
      void port.setBreakpoints(path, next.get(path) ?? []);
    }
  };

  const onThread = async (work: (threadId: number) => Promise<IpcResult<null>>) => {
    if (state.threadId === null) return;
    const result = await work(state.threadId);
    if (!result.ok) set({ error: result.error.message });
  };

  return {
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    getState: () => state,
    async start(path) {
      set({
        phase: "starting",
        program: path,
        threadId: null,
        pausedBecause: null,
        frames: [],
        frameId: null,
        scopes: [],
        output: [],
        error: null,
      });
      const result = await port.start(path, list());
      if (result.ok) {
        // It may have stopped already, at a breakpoint on the first line.
        if (state.phase === "starting") set({ phase: "running" });
      } else {
        set({ phase: "idle", error: result.error.message });
      }
      return result;
    },
    async stop() {
      await port.stop();
      set({
        phase: "ended",
        threadId: null,
        pausedBecause: null,
        frames: [],
        frameId: null,
        scopes: [],
      });
    },
    resume: () => onThread((thread) => port.resume(thread)),
    step: (how) => onThread((thread) => port.resume(thread, how)),
    pause: () => onThread((thread) => port.pause(thread)),
    toggleBreakpoint(path, line) {
      const lines = state.breakpoints.get(path) ?? [];
      setLines(path, lines.includes(line) ? lines.filter((at) => at !== line) : [...lines, line]);
    },
    removeBreakpoint(path, line) {
      setLines(
        path,
        (state.breakpoints.get(path) ?? []).filter((at) => at !== line),
      );
    },
    async selectFrame(frameId) {
      const frame = state.frames.find((candidate) => candidate.id === frameId);
      if (!frame) return;
      set({ frameId, scopes: [] });
      show(frame);
      await loadScopes(frameId);
    },
    async children(reference) {
      const result = await port.variables(reference);
      return result.ok ? result.value : [];
    },
    async evaluate(expression) {
      write("input", `> ${expression}`);
      const result = await port.evaluate(expression, state.frameId ?? undefined);
      write(result.ok ? "result" : "stderr", result.ok ? result.value : result.error.message);
    },
    executionLine(path) {
      if (state.phase !== "paused") return null;
      const frame = state.frames.find((candidate) => candidate.id === state.frameId);
      return frame?.source?.kind === "workspace" && frame.source.path === path ? frame.line : null;
    },
    dispose() {
      stopListening();
      if (state.phase === "running" || state.phase === "paused" || state.phase === "starting") {
        void port.stop();
      }
    },
  };
}
