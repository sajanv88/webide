import type {
  AppCommandId,
  AppEvents,
  GitBranchDto,
  GitFileDto,
  GitPort,
  GitStateDto,
  IpcResult,
} from "@repo/contracts";
import { createCommandRegistry, createEventBus } from "@repo/kernel";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { BranchPicker, BranchStatus } from "./branch-picker";
import { createGitStore } from "./git-store";
import { validBranchName } from "./names";
import { SourceControlView } from "./source-control-view";

const ok = <T,>(value: T): IpcResult<T> => ({ ok: true, value });
const done = ok(null);

const head = { branch: "main", commit: "abc1234", upstream: "origin/main", ahead: 1, behind: 0 };
const file = (path: string, staged: GitFileDto["staged"], unstaged: GitFileDto["unstaged"]) => ({
  path,
  from: null,
  staged,
  unstaged,
});

/** A repository in memory: staging moves files between the lists. */
function fakeRepo(files: GitFileDto[], overrides: Partial<GitPort> = {}) {
  let current = [...files];
  const calls: string[] = [];
  const port: GitPort = {
    state: async (): Promise<IpcResult<GitStateDto>> =>
      ok({ kind: "repository", head, files: current, truncated: false }),
    diff: async (path, side) => {
      calls.push(`diff ${path} ${side}`);
      return ok({ kind: "text", original: "before\n", modified: "after\n" });
    },
    stage: async (paths) => {
      calls.push(`stage ${paths.join(",")}`);
      current = current.map((f) =>
        paths.includes(f.path)
          ? { ...f, staged: f.unstaged === "untracked" ? "added" : f.unstaged, unstaged: null }
          : f,
      );
      return done;
    },
    unstage: async (paths) => {
      calls.push(`unstage ${paths.join(",")}`);
      current = current.map((f) =>
        paths.includes(f.path) ? { ...f, unstaged: f.staged, staged: null } : f,
      );
      return done;
    },
    discard: async (paths) => {
      calls.push(`discard ${paths.join(",")}`);
      current = current.filter((f) => !paths.includes(f.path));
      return done;
    },
    commit: async (message) => {
      calls.push(`commit ${message}`);
      current = current.filter((f) => f.staged === null);
      return done;
    },
    branches: async (): Promise<IpcResult<GitBranchDto[]>> =>
      ok([
        { name: "main", current: true },
        { name: "feature/x", current: false },
      ]),
    switchBranch: async (name, create) => {
      calls.push(`switch ${name} ${create}`);
      return done;
    },
    init: async () => {
      calls.push("init");
      return done;
    },
    ...overrides,
  };
  return { port, calls };
}

function renderView(port: GitPort) {
  const store = createGitStore(port);
  const registry = createCommandRegistry<AppCommandId>();
  const bus = createEventBus<AppEvents>();
  render(
    <SourceControlView
      store={store}
      readDiff={port.diff}
      registry={registry}
      bus={bus}
      viewId="git"
      renderDiff={(props) => (
        <section aria-label={props.label}>
          {props.original}→{props.modified}
        </section>
      )}
    />,
  );
  return { store, registry, bus };
}

const list = (name: string) => screen.findByRole("grid", { name: new RegExp(`^${name}`) });
const rows = async (name: string) =>
  within(await list(name))
    .getAllByRole("row")
    .map((row) => row.textContent);

describe("validBranchName", () => {
  it("takes what git takes", () => {
    for (const good of ["main", "feature/search", "fix-12", "release/2026.11"]) {
      expect(validBranchName(good), good).toBe(true);
    }
    for (const bad of [
      "",
      "-x",
      "/x",
      "x/",
      "x.",
      "x.lock",
      "a..b",
      "a b",
      "a~b",
      ".hidden",
      "a//b",
    ]) {
      expect(validBranchName(bad), bad).toBe(false);
    }
  });
});

describe("createGitStore", () => {
  it("sorts files into conflicts, staged and changes; a file can be in two", async () => {
    const { port } = fakeRepo([
      file("both.ts", "modified", "modified"),
      file("new.ts", null, "untracked"),
      file("merge.ts", null, "conflicted"),
      file("ready.ts", "added", null),
    ]);
    const store = createGitStore(port);
    await store.refresh();
    const view = store.getState().view;
    if (view.status !== "repository") throw new Error(view.status);
    expect(view.conflicts.map((f) => f.path)).toEqual(["merge.ts"]);
    expect(view.staged.map((f) => f.path)).toEqual(["both.ts", "ready.ts"]);
    expect(view.changes.map((f) => f.path)).toEqual(["both.ts", "new.ts"]);
  });

  it("folds refreshes asked for while one runs into one more", async () => {
    const state = vi.fn(
      async (): Promise<IpcResult<GitStateDto>> => ok({ kind: "notARepository" }),
    );
    const store = createGitStore({ ...fakeRepo([]).port, state });
    await Promise.all([store.refresh(), store.refresh(), store.refresh()]);
    expect(state).toHaveBeenCalledTimes(2);
    expect(store.getState().view.status).toBe("notARepository");
  });

  it("reads the state again after each operation", async () => {
    const { port, calls } = fakeRepo([
      file("a.ts", null, "modified"),
      file("b.ts", null, "untracked"),
    ]);
    const store = createGitStore(port);
    await store.refresh();
    await store.stageAll();
    expect(calls).toEqual(["stage a.ts,b.ts"]);
    const view = store.getState().view;
    expect(view.status === "repository" && view.staged.length).toBe(2);
  });
});

describe("SourceControlView", () => {
  it("shows the branch and each list, named, with a word for each change", async () => {
    const { port } = fakeRepo([
      file("src/a.ts", "modified", null),
      file("notes.md", null, "untracked"),
    ]);
    renderView(port);
    expect(await screen.findByText("main")).toBeTruthy();
    expect(screen.getByText(", 1 to push")).toBeTruthy();
    // The letter is drawn; the word is said.
    expect(await rows("Staged changes")).toEqual(["a.tssrcM, modified"]);
    expect(await rows("Changes")).toEqual(["notes.mdU, untracked"]);
    expect(screen.getByRole("button", { name: "Commit 1 file" })).toBeTruthy();
  });

  it("stages, unstages and discards (after asking)", async () => {
    const { port, calls } = fakeRepo([file("a.ts", null, "modified"), file("b.ts", "added", null)]);
    renderView(port);
    fireEvent.click(await screen.findByRole("button", { name: "Stage a.ts" }));
    await waitFor(() => expect(calls).toContain("stage a.ts"));
    fireEvent.click(await screen.findByRole("button", { name: "Unstage b.ts" }));
    await waitFor(() => expect(calls).toContain("unstage b.ts"));

    fireEvent.click(await screen.findByRole("button", { name: "Discard changes to b.ts" }));
    const ask = await screen.findByRole("alertdialog", { name: "Discard changes to b.ts?" });
    expect(calls).not.toContain("discard b.ts");
    fireEvent.click(within(ask).getByRole("button", { name: "Discard changes" }));
    await waitFor(() => expect(calls).toContain("discard b.ts"));
  });

  it("opens a file's changes in a dialog with the app's diff view", async () => {
    const { port, calls } = fakeRepo([file("src/a.ts", null, "modified")]);
    const { bus } = renderView(port);
    const opened = vi.fn();
    bus.on("file.openRequested", opened);
    const row = within(await list("Changes")).getByRole("row");
    act(() => row.focus());
    fireEvent.keyDown(row, { key: "Enter" });
    const dialog = await screen.findByRole("dialog", { name: "Changes to src/a.ts" });
    expect(calls).toContain("diff src/a.ts unstaged");
    expect((await within(dialog).findByLabelText("Changes to src/a.ts")).textContent).toBe(
      "before\n→after\n",
    );
    fireEvent.keyDown(dialog, { key: "Escape" });
    // Focus goes back to the row it was opened from.
    await waitFor(() => expect(document.activeElement).toBe(row));

    fireEvent.keyDown(row, { key: "Enter" });
    const again = await screen.findByRole("dialog", { name: "Changes to src/a.ts" });
    fireEvent.click(within(again).getByRole("button", { name: "Open file" }));
    expect(opened).toHaveBeenCalledWith({ path: "src/a.ts" });
  });

  it("commits with Ctrl+Enter, and says what's missing first", async () => {
    const { port, calls } = fakeRepo([file("a.ts", null, "modified")]);
    renderView(port);
    const box = await screen.findByRole("textbox", { name: "Commit message" });

    fireEvent.click(screen.getByRole("button", { name: "Commit" }));
    expect(await screen.findByText("Write a message for the commit.")).toBeTruthy();

    fireEvent.change(box, { target: { value: "Fix it" } });
    fireEvent.keyDown(box, { key: "Enter", ctrlKey: true });
    expect(await screen.findByText("Stage the changes to commit first.")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Stage a.ts" }));
    await screen.findByRole("button", { name: "Commit 1 file" });
    fireEvent.keyDown(box, { key: "Enter", ctrlKey: true });
    await waitFor(() => expect(calls).toContain("commit Fix it"));
    await waitFor(() => expect((box as HTMLTextAreaElement).value).toBe(""));
    expect(await screen.findByText("No changes.")).toBeTruthy();
  });

  it("shows git's refusal on the message field", async () => {
    const { port } = fakeRepo([file("a.ts", "modified", null)], {
      commit: async () => ({
        ok: false,
        error: { code: "git", message: "pre-commit hook failed" },
      }),
    });
    renderView(port);
    const box = await screen.findByRole("textbox", { name: "Commit message" });
    fireEvent.change(box, { target: { value: "x" } });
    fireEvent.click(screen.getByRole("button", { name: "Commit 1 file" }));
    expect(await screen.findByText("pre-commit hook failed")).toBeTruthy();
    await waitFor(() => expect(box.getAttribute("aria-invalid")).toBe("true"));
  });

  it("offers to make a repository, and says when git is missing", async () => {
    const { port, calls } = fakeRepo([], { state: async () => ok({ kind: "notARepository" }) });
    const { registry } = renderView(port);
    fireEvent.click(await screen.findByRole("button", { name: "Initialize repository" }));
    await waitFor(() => expect(calls).toContain("init"));
    expect(registry.get("git.init")).toBeTruthy();
    expect(registry.get("git.commit")).toBeUndefined();
  });

  it("says when git isn't installed", async () => {
    const { port } = fakeRepo([], { state: async () => ok({ kind: "unavailable" }) });
    renderView(port);
    expect(await screen.findByText("Git isn't installed")).toBeTruthy();
  });

  it("is off in Restricted Mode, with a way to trust the folder", async () => {
    const { port } = fakeRepo([], { state: async () => ok({ kind: "restricted" }) });
    const { registry } = renderView(port);
    const manage = vi.fn();
    registry.register({ id: "workspace.manageTrust", title: "Manage", run: manage });
    expect(await screen.findByText("Source control is off in Restricted Mode")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Manage Workspace Trust" }));
    expect(manage).toHaveBeenCalledOnce();
  });

  it("reads the state again once changes on disk settle", async () => {
    const state = vi.fn(fakeRepo([]).port.state);
    const { bus } = renderView({ ...fakeRepo([]).port, state });
    await waitFor(() => expect(state).toHaveBeenCalledTimes(1));
    act(() => {
      bus.emit("workspace.filesChanged", { changes: [], rescan: true });
      bus.emit("workspace.filesChanged", { changes: [], rescan: true });
    });
    await waitFor(() => expect(state).toHaveBeenCalledTimes(2), { timeout: 2000 });
  });
});

describe("BranchPicker", () => {
  async function openPicker() {
    const { port, calls } = fakeRepo([]);
    const store = createGitStore(port);
    await store.refresh();
    const registry = createCommandRegistry<AppCommandId>();
    render(
      <>
        <BranchPicker store={store} registry={registry} />
        <BranchStatus store={store} registry={registry} />
      </>,
    );
    fireEvent.click(screen.getByRole("button", { name: /^Branch main, 1 to push/ }));
    const dialog = await screen.findByRole("dialog", { name: "Switch branch" });
    return { dialog, calls };
  }

  it("lists the branches and switches to the one chosen", async () => {
    const { dialog, calls } = await openPicker();
    const options = await within(dialog).findAllByRole("option");
    expect(options.map((o) => o.textContent)).toEqual(["maincurrent", "feature/x"]);
    expect(options[0]?.getAttribute("aria-disabled")).toBe("true");
    fireEvent.click(options[1] as HTMLElement);
    await waitFor(() => expect(calls).toContain("switch feature/x false"));
  });

  it("offers to create a branch with a valid new name only", async () => {
    const { dialog, calls } = await openPicker();
    const input = within(dialog).getByRole("combobox");
    fireEvent.change(input, { target: { value: "bad name" } });
    expect(
      await within(dialog).findAllByText("“bad name” can't be a branch name."),
    ).not.toHaveLength(0);
    fireEvent.change(input, { target: { value: "feature/new" } });
    fireEvent.click(
      await within(dialog).findByText("Create branch “feature/new” and switch to it"),
    );
    await waitFor(() => expect(calls).toContain("switch feature/new true"));
  });
});
