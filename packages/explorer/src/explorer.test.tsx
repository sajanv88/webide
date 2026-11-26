import type {
  AppCommandId,
  AppEvents,
  DirListingDto,
  ExplorerPort,
  FileNodeDto,
  IpcResult,
  WorkspaceDto,
} from "@repo/contracts";
import { createCommandRegistry, createEventBus } from "@repo/kernel";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { Explorer } from "./explorer";

const workspace: WorkspaceDto = { id: "1", name: "project", path: "/tmp/project", trusted: true };

function file(path: string): FileNodeDto {
  const name = path.split("/").pop() ?? path;
  return { name, path, kind: "file", isSymlink: false, size: 1 };
}
function dir(path: string): FileNodeDto {
  const name = path.split("/").pop() ?? path;
  return { name, path, kind: "directory", isSymlink: false, size: null };
}
function listing(path: string, entries: FileNodeDto[]): IpcResult<DirListingDto> {
  return { ok: true, value: { path, entries, truncated: false, skipped: 0 } };
}

/** A port answering from a table, recording every call. */
function fakePort(table: Record<string, IpcResult<DirListingDto>>) {
  const calls: string[] = [];
  const port: ExplorerPort = {
    listDirectory: async (path) => {
      calls.push(path);
      return (
        table[path] ?? {
          ok: false,
          error: { code: "notFound", message: `no listing for "${path}"` },
        }
      );
    },
  };
  return { port, calls };
}

function renderExplorer(port: ExplorerPort, pageSize?: number) {
  const registry = createCommandRegistry<AppCommandId>();
  const bus = createEventBus<AppEvents>();
  const view = render(
    <Explorer
      source={{ workspace, port }}
      registry={registry}
      bus={bus}
      {...(pageSize ? { pageSize } : {})}
    />,
  );
  return { registry, bus, ...view };
}

const row = (name: string) => screen.findByRole("treeitem", { name: new RegExp(`^${name}`) });

/** Chooses a row the way the keyboard does: focus it, press Enter. */
function choose(item: HTMLElement) {
  act(() => item.focus());
  fireEvent.keyDown(item, { key: "Enter" });
}

describe("Explorer", () => {
  it("lists the root on mount and opens folders lazily", async () => {
    const { port, calls } = fakePort({
      "": listing("", [dir("src"), file("README.md")]),
      src: listing("src", [file("src/main.ts")]),
    });
    renderExplorer(port);

    const src = await row("src");
    expect(screen.getByRole("treeitem", { name: "README.md" })).toBeTruthy();
    expect(src.getAttribute("aria-expanded")).toBe("false");
    expect(calls).toEqual([""]);

    act(() => src.focus());
    fireEvent.keyDown(src, { key: "ArrowRight" });

    expect(await row("main.ts")).toBeTruthy();
    expect(calls).toEqual(["", "src"]);
  });

  it("shows empty folders and errors as rows, and retries an error when reopened", async () => {
    const { port, calls } = fakePort({
      "": listing("", [dir("empty"), dir("broken")]),
      empty: listing("empty", []),
    });
    renderExplorer(port);

    const empty = await row("empty");
    act(() => empty.focus());
    fireEvent.keyDown(empty, { key: "ArrowRight" });
    expect(await row("Empty folder")).toBeTruthy();

    const broken = await row("broken");
    act(() => broken.focus());
    fireEvent.keyDown(broken, { key: "ArrowRight" });
    expect(await row("Couldn't open this folder")).toBeTruthy();

    fireEvent.keyDown(broken, { key: "ArrowLeft" });
    fireEvent.keyDown(broken, { key: "ArrowRight" });
    await waitFor(() => expect(calls.filter((path) => path === "broken")).toHaveLength(2));
  });

  it("draws a big folder a page at a time", async () => {
    const files = Array.from({ length: 7 }, (_, index) => file(`f${index}.ts`));
    const { port } = fakePort({ "": listing("", files) });
    renderExplorer(port, 3);

    const more = await row("Show 3 more \\(4 not shown\\)");
    expect(screen.queryByRole("treeitem", { name: "f3.ts" })).toBeNull();

    choose(more);
    expect(await row("f5.ts")).toBeTruthy();
    expect(await row("Show 1 more \\(1 not shown\\)")).toBeTruthy();
  });

  it("emits fileActivated when a file is chosen, not for a folder", async () => {
    const { port } = fakePort({ "": listing("", [dir("src"), file("README.md")]) });
    const { bus } = renderExplorer(port);
    const activated = vi.fn();
    bus.on("explorer.fileActivated", activated);

    choose(await row("src"));
    choose(await row("README.md"));

    expect(activated).toHaveBeenCalledTimes(1);
    expect(activated).toHaveBeenCalledWith({ path: "README.md" });
  });

  it("registers refresh and collapse-all while mounted; refresh reloads open folders", async () => {
    const { port, calls } = fakePort({
      "": listing("", [dir("src")]),
      src: listing("src", [file("src/a.ts")]),
    });
    const { registry, unmount } = renderExplorer(port);
    const src = await row("src");
    act(() => src.focus());
    fireEvent.keyDown(src, { key: "ArrowRight" });
    await row("a.ts");

    await act(async () => {
      await registry.execute("explorer.refresh");
    });
    await waitFor(() => expect(calls).toEqual(["", "src", "", "src"]));

    await act(async () => {
      await registry.execute("explorer.collapseAll");
    });
    expect((await row("src")).getAttribute("aria-expanded")).toBe("false");

    unmount();
    expect(registry.get("explorer.refresh")).toBeUndefined();
  });

  it("relists only the shown folders a change on disk touches", async () => {
    const table = {
      "": listing("", [dir("src"), dir("docs"), file("README.md")]),
      src: listing("src", [file("src/a.ts")]),
      docs: listing("docs", [file("docs/guide.md")]),
    };
    const { port, calls } = fakePort(table);
    const { bus } = renderExplorer(port);
    const src = await row("src");
    act(() => src.focus());
    fireEvent.keyDown(src, { key: "ArrowRight" });
    await row("a.ts");
    expect(calls).toEqual(["", "src"]);

    // A file added to src, and one changed in docs, which was never opened.
    table.src = listing("src", [file("src/a.ts"), file("src/b.ts")]);
    act(() =>
      bus.emit("workspace.filesChanged", {
        changes: [
          { kind: "created", path: "src/b.ts" },
          { kind: "modified", path: "docs/guide.md" },
        ],
        rescan: false,
      }),
    );
    await row("b.ts");
    expect(calls).toEqual(["", "src", "src"]);

    // A rescan lists every shown folder again.
    act(() => bus.emit("workspace.filesChanged", { changes: [], rescan: true }));
    await waitFor(() => expect(calls).toEqual(["", "src", "src", "", "src"]));
  });

  it("forgets a removed folder, so it isn't shown open when it comes back", async () => {
    const table: Record<string, IpcResult<DirListingDto>> = {
      "": listing("", [dir("src")]),
      src: listing("src", [file("src/a.ts")]),
    };
    const { port } = fakePort(table);
    const { bus } = renderExplorer(port);
    const src = await row("src");
    act(() => src.focus());
    fireEvent.keyDown(src, { key: "ArrowRight" });
    await row("a.ts");

    table[""] = listing("", []);
    act(() =>
      bus.emit("workspace.filesChanged", {
        changes: [{ kind: "removed", path: "src" }],
        rescan: false,
      }),
    );
    await waitFor(() => expect(screen.queryByRole("treeitem", { name: /^src/ })).toBeNull());

    table[""] = listing("", [dir("src")]);
    act(() =>
      bus.emit("workspace.filesChanged", {
        changes: [{ kind: "created", path: "src" }],
        rescan: false,
      }),
    );
    expect((await row("src")).getAttribute("aria-expanded")).toBe("false");
  });
});
