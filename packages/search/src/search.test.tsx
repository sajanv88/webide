import type {
  AppCommandId,
  AppEvents,
  FileListDto,
  IpcResult,
  SearchPort,
  SearchTextArgs,
  TextSearchDto,
} from "@repo/contracts";
import { createCommandRegistry, createEventBus } from "@repo/kernel";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { matchFile, matchFiles } from "./fuzzy";
import { merge } from "./highlight";
import { QuickOpen } from "./quick-open";
import { SearchView, splitGlobs } from "./search-view";

const ok = <T,>(value: T): IpcResult<T> => ({ ok: true, value });

const found: TextSearchDto = {
  files: [
    {
      path: "src/app.ts",
      lines: [
        { line: 3, column: 7, preview: "const needle = 1;", spans: [{ start: 6, end: 12 }] },
        { line: 9, column: 1, preview: "needle();", spans: [{ start: 0, end: 6 }] },
      ],
    },
    {
      path: "README.md",
      lines: [{ line: 1, column: 3, preview: "# needle", spans: [{ start: 2, end: 8 }] }],
    },
  ],
  truncated: false,
  cancelled: false,
};

function fakePort(overrides: Partial<SearchPort> = {}) {
  const queries: Omit<SearchTextArgs, "workspaceId">[] = [];
  const port: SearchPort = {
    searchText: async (query) => {
      queries.push(query);
      return ok(found);
    },
    listFiles: async () => ok<FileListDto>({ files: [], truncated: false }),
    ...overrides,
  };
  return { port, queries };
}

describe("matchFiles", () => {
  it("matches characters in order, ignoring case and spaces", () => {
    expect(matchFile("src/document-tabs.tsx", "dtabs")?.positions).toEqual([4, 13, 14, 15, 16]);
    expect(matchFile("src/document-tabs.tsx", "D TABS")).not.toBeNull();
    expect(matchFile("src/document-tabs.tsx", "xyz")).toBeNull();
  });

  it("ranks a match in the file's name above one spread over its folders", () => {
    const files = ["tabs/src/index.ts", "src/tabs.ts", "a/t/a/b/s.ts"];
    expect(matchFiles(files, "tabs", 10).map((match) => match.path)).toEqual([
      "src/tabs.ts",
      "tabs/src/index.ts",
      "a/t/a/b/s.ts",
    ]);
  });

  it("keeps the order given and the limit when nothing is typed", () => {
    expect(matchFiles(["b", "a", "c"], " ", 2).map((match) => match.path)).toEqual(["b", "a"]);
  });
});

describe("helpers", () => {
  it("splits globs at commas and drops blanks", () => {
    expect(splitGlobs(" src/**, ,*.ts ")).toEqual(["src/**", "*.ts"]);
  });

  it("merges overlapping ranges and clamps them", () => {
    expect(
      merge(
        [
          [4, 6],
          [0, 2],
          [1, 3],
          [8, 20],
          [5, 5],
        ],
        10,
      ),
    ).toEqual([
      [0, 3],
      [4, 6],
      [8, 10],
    ]);
  });
});

function renderSearch(port: SearchPort) {
  const registry = createCommandRegistry<AppCommandId>();
  const bus = createEventBus<AppEvents>();
  render(<SearchView port={port} registry={registry} bus={bus} viewId="search" />);
  return { registry, bus };
}

describe("SearchView", () => {
  it("searches after typing pauses, and shows files and lines with matches marked", async () => {
    const { port, queries } = fakePort();
    renderSearch(port);
    expect(screen.getByRole("search", { name: "Find in files" })).toBeTruthy();

    fireEvent.change(screen.getByRole("textbox", { name: "Search" }), {
      target: { value: "needle" },
    });
    const status = await screen.findByText("3 results in 2 files.");
    expect(status.getAttribute("role")).toBe("status");
    expect(queries).toEqual([
      {
        pattern: "needle",
        matchCase: false,
        wholeWord: false,
        isRegex: false,
        include: [],
        exclude: [],
      },
    ]);

    const tree = screen.getByRole("tree", { name: "Search results" });
    const app = within(tree).getByRole("treeitem", { name: /app\.ts/ });
    expect(app.textContent).toContain("2 matches");
    const marks = tree.querySelectorAll("mark");
    expect([...marks].map((mark) => mark.textContent)).toEqual(["needle", "needle", "needle"]);
  });

  it("passes the options and the file globs", async () => {
    const { port, queries } = fakePort();
    renderSearch(port);
    fireEvent.click(screen.getByRole("button", { name: "Match case" }));
    fireEvent.click(screen.getByRole("button", { name: "Use regular expression" }));
    fireEvent.click(screen.getByRole("button", { name: "Files to include or exclude" }));
    fireEvent.change(screen.getByLabelText("Files to include"), { target: { value: "src/**" } });
    fireEvent.change(screen.getByLabelText("Files to exclude"), {
      target: { value: "*.test.ts, dist/" },
    });
    fireEvent.change(screen.getByRole("textbox", { name: "Search" }), {
      target: { value: "ne+dle" },
    });
    await waitFor(() => expect(queries).toHaveLength(1));
    expect(queries[0]).toEqual({
      pattern: "ne+dle",
      matchCase: true,
      wholeWord: false,
      isRegex: true,
      include: ["src/**"],
      exclude: ["*.test.ts", "dist/"],
    });
  });

  it("opens a line's file at its position when the line is chosen", async () => {
    const { port } = fakePort();
    const { bus } = renderSearch(port);
    const opened = vi.fn();
    bus.on("file.openRequested", opened);
    fireEvent.change(screen.getByRole("textbox", { name: "Search" }), {
      target: { value: "needle" },
    });
    const line = await screen.findByRole("treeitem", { name: /const needle = 1;/ });
    act(() => line.focus());
    fireEvent.keyDown(line, { key: "Enter" });
    expect(opened).toHaveBeenCalledWith({ path: "src/app.ts", line: 3, column: 7 });
  });

  it("shows an invalid pattern on the field, and says when nothing matched", async () => {
    const { port } = fakePort({
      searchText: async (query) =>
        query.pattern === "("
          ? {
              ok: false,
              error: { code: "invalidQuery", message: "invalid search: unclosed group" },
            }
          : ok({ files: [], truncated: false, cancelled: false }),
    });
    renderSearch(port);
    const input = screen.getByRole("textbox", { name: "Search" });
    fireEvent.change(input, { target: { value: "(" } });
    expect(await screen.findByText("invalid search: unclosed group")).toBeTruthy();
    // The field marks its control a moment after the message is drawn.
    await waitFor(() => expect(input.getAttribute("aria-invalid")).toBe("true"));

    fireEvent.change(input, { target: { value: "zzz" } });
    expect(await screen.findByText("No results for “zzz”.")).toBeTruthy();
  });

  it("says when the search stopped at its limit", async () => {
    const { port } = fakePort({ searchText: async () => ok({ ...found, truncated: true }) });
    renderSearch(port);
    fireEvent.change(screen.getByRole("textbox", { name: "Search" }), {
      target: { value: "needle" },
    });
    expect(await screen.findByText(/The search stopped there/)).toBeTruthy();
  });

  it("registers Find in Files, which shows the view and focuses the field", async () => {
    const { port } = fakePort();
    const { registry } = renderSearch(port);
    const shown = vi.fn();
    registry.register({ id: "view.showView", title: "Show View", palette: false, run: shown });
    await act(() => registry.execute("search.findInFiles"));
    expect(shown).toHaveBeenCalledWith("search");
    await waitFor(() =>
      expect(document.activeElement).toBe(screen.getByRole("textbox", { name: "Search" })),
    );
  });
});

describe("QuickOpen", () => {
  const files = ["src/app.ts", "src/document-tabs.tsx", "README.md"];

  function renderQuickOpen(listFiles: SearchPort["listFiles"]) {
    const registry = createCommandRegistry<AppCommandId>();
    const bus = createEventBus<AppEvents>();
    const { port } = fakePort({ listFiles });
    render(<QuickOpen port={port} registry={registry} bus={bus} />);
    return { registry, bus };
  }

  it("lists the files, matches what's typed, and opens the chosen one", async () => {
    const listFiles = vi.fn(async () => ok({ files, truncated: false }));
    const { registry, bus } = renderQuickOpen(listFiles);
    const opened = vi.fn();
    bus.on("file.openRequested", opened);

    await act(() => registry.execute("search.goToFile"));
    const dialog = await screen.findByRole("dialog", { name: "Go to file" });
    expect(await within(dialog).findByText("README.md")).toBeTruthy();

    fireEvent.change(within(dialog).getByRole("combobox"), { target: { value: "dtabs" } });
    const options = await within(dialog).findAllByRole("option");
    expect(options).toHaveLength(1);
    expect(options[0]?.textContent).toBe("document-tabs.tsxsrc");
    fireEvent.click(options[0] as HTMLElement);
    expect(opened).toHaveBeenCalledWith({ path: "src/document-tabs.tsx" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(listFiles).toHaveBeenCalledTimes(1);
  });

  it("reads the list again after files are added, not after edits", async () => {
    const listFiles = vi.fn(async () => ok({ files, truncated: false }));
    const { registry, bus } = renderQuickOpen(listFiles);
    await act(() => registry.execute("search.goToFile"));
    await waitFor(() => expect(listFiles).toHaveBeenCalledTimes(1));

    act(() => {
      bus.emit("workspace.filesChanged", {
        changes: [{ kind: "modified", path: "src/app.ts" }],
        rescan: false,
      });
    });
    expect(listFiles).toHaveBeenCalledTimes(1);
    act(() => {
      bus.emit("workspace.filesChanged", {
        changes: [{ kind: "created", path: "src/new.ts" }],
        rescan: false,
      });
    });
    await waitFor(() => expect(listFiles).toHaveBeenCalledTimes(2));
  });

  it("says when the files couldn't be listed", async () => {
    const { registry } = renderQuickOpen(async () => ({
      ok: false,
      error: { code: "notFound", message: "not found" },
    }));
    await act(() => registry.execute("search.goToFile"));
    // Shown in the list, and said by its status line too.
    expect(await screen.findAllByText("Couldn't list the files: not found")).not.toHaveLength(0);
  });
});
