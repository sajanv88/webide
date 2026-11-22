import type { GitBranchDto, GitFileDto, GitHeadDto, GitPort, IpcResult } from "@repo/contracts";

/** What the source control view shows. */
export type GitView =
  | { readonly status: "loading" }
  | { readonly status: "unavailable" }
  | { readonly status: "notARepository" }
  | { readonly status: "failed"; readonly message: string }
  | {
      readonly status: "repository";
      readonly head: GitHeadDto;
      /** Changes in the index: what the next commit has. */
      readonly staged: readonly GitFileDto[];
      /** Changes in the working tree not staged, untracked files included. */
      readonly changes: readonly GitFileDto[];
      /** Files both sides of a merge changed. */
      readonly conflicts: readonly GitFileDto[];
      readonly truncated: boolean;
    };

export interface GitState {
  readonly view: GitView;
  /** An operation (stage, commit…) is running. */
  readonly busy: boolean;
}

export interface GitStore {
  subscribe(listener: () => void): () => void;
  getState(): GitState;
  /** Reads git's state again. Calls made while one runs are folded into one more. */
  refresh(): Promise<void>;
  stage(paths: readonly string[]): Promise<IpcResult<null>>;
  unstage(paths: readonly string[]): Promise<IpcResult<null>>;
  discard(paths: readonly string[]): Promise<IpcResult<null>>;
  stageAll(): Promise<IpcResult<null>>;
  unstageAll(): Promise<IpcResult<null>>;
  commit(message: string): Promise<IpcResult<null>>;
  branches(): Promise<IpcResult<GitBranchDto[]>>;
  switchBranch(name: string, create: boolean): Promise<IpcResult<null>>;
  init(): Promise<IpcResult<null>>;
}

/** The view of a repository, with each file in the list it belongs to. */
export function toView(
  head: GitHeadDto,
  files: readonly GitFileDto[],
  truncated: boolean,
): GitView {
  const conflicts = files.filter((file) => file.unstaged === "conflicted");
  const staged = files.filter((file) => file.staged !== null && file.unstaged !== "conflicted");
  const changes = files.filter((file) => file.unstaged !== null && file.unstaged !== "conflicted");
  return { status: "repository", head, staged, changes, conflicts, truncated };
}

/** Git for one workspace: its state, kept current, and the operations on it. */
export function createGitStore(port: GitPort): GitStore {
  let state: GitState = { view: { status: "loading" }, busy: false };
  const listeners = new Set<() => void>();
  const set = (next: Partial<GitState>) => {
    state = { ...state, ...next };
    for (const listener of [...listeners]) listener();
  };

  let running: Promise<void> | null = null;
  let again = false;
  const read = async () => {
    const result = await port.state();
    if (!result.ok) {
      set({ view: { status: "failed", message: result.error.message } });
      return;
    }
    const value = result.value;
    switch (value.kind) {
      case "unavailable":
        set({ view: { status: "unavailable" } });
        break;
      case "notARepository":
        set({ view: { status: "notARepository" } });
        break;
      case "repository":
        set({ view: toView(value.head, value.files, value.truncated) });
        break;
    }
  };
  const refresh = (): Promise<void> => {
    if (running) {
      again = true;
      return running;
    }
    running = (async () => {
      do {
        again = false;
        await read();
      } while (again);
      running = null;
    })();
    return running;
  };

  /** Runs an operation, then reads the state it left. */
  const operate = async <T>(work: () => Promise<IpcResult<T>>): Promise<IpcResult<T>> => {
    set({ busy: true });
    try {
      return await work();
    } finally {
      set({ busy: false });
      await refresh();
    }
  };
  const nothing = (): Promise<IpcResult<null>> => Promise.resolve({ ok: true, value: null });
  const paths = (list: () => readonly GitFileDto[]) => list().map((file) => file.path);
  const repo = () => (state.view.status === "repository" ? state.view : null);

  return {
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    getState: () => state,
    refresh,
    stage: (list) => (list.length ? operate(() => port.stage(list)) : nothing()),
    unstage: (list) => (list.length ? operate(() => port.unstage(list)) : nothing()),
    discard: (list) => (list.length ? operate(() => port.discard(list)) : nothing()),
    stageAll() {
      const list = paths(() => repo()?.changes ?? []);
      return list.length ? operate(() => port.stage(list)) : nothing();
    },
    unstageAll() {
      const list = paths(() => repo()?.staged ?? []);
      return list.length ? operate(() => port.unstage(list)) : nothing();
    },
    commit: (message) => operate(() => port.commit(message)),
    branches: () => port.branches(),
    switchBranch: (name, create) => operate(() => port.switchBranch(name, create)),
    init: () => operate(() => port.init()),
  };
}
