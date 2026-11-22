import type {
  AppCommandId,
  AppEvents,
  DocumentsPort,
  IpcResult,
  ReadFileDto,
} from "@repo/contracts";
import { createCommandRegistry, createEventBus } from "@repo/kernel";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useEffect } from "react";
import { describe, expect, it } from "vitest";
import { DocumentTabs, type EditorSlotHandle } from "./document-tabs";
import { createDocuments } from "./documents";

const stamp = { size: 4, modifiedMs: 1 };
const ok = <T,>(value: T): IpcResult<T> => ({ ok: true, value });

function setup(files: Record<string, ReadFileDto>) {
  const written: string[] = [];
  const port: DocumentsPort = {
    readFile: async (path) =>
      files[path] ? ok(files[path]) : { ok: false, error: { code: "notFound", message: "gone" } },
    writeFile: async (path, text) => {
      written.push(`${path}:${text}`);
      return ok({ size: text.length, modifiedMs: 2 });
    },
  };
  const documents = createDocuments(port);
  const registry = createCommandRegistry<AppCommandId>();
  const bus = createEventBus<AppEvents>();
  const view = render(
    <DocumentTabs
      documents={documents}
      registry={registry}
      bus={bus}
      empty={<p>Nothing open</p>}
      renderEditor={(props) => (
        // A stand-in for the editor surface, which isn't this package's.
        <textarea
          key={props.documentId}
          aria-label={`Editor for ${props.path}`}
          defaultValue={props.initialValue}
          readOnly={props.readOnly}
          onChange={(event) => props.onChange(event.target.value)}
        />
      )}
    />,
  );
  const open = async (path: string) => {
    await act(async () => {
      bus.emit("explorer.fileActivated", { path });
      await Promise.resolve();
    });
    await waitFor(() =>
      expect(
        documents
          .getState()
          .tabs.some((tab) => tab.path === path && tab.content.kind !== "loading"),
      ).toBe(true),
    );
  };
  return { ...view, documents, registry, bus, written, open };
}

const textFile = (path: string, text: string, extra: Partial<ReadFileDto> = {}): ReadFileDto =>
  ({ kind: "text", path, text, stamp, lineEnding: "lf", readOnly: null, ...extra }) as ReadFileDto;

/** An editor whose handle comes a moment after it's drawn, as one that loads its code does. */
function LateEditor({
  calls,
  onHandle,
}: {
  calls: string[];
  onHandle: (handle: EditorSlotHandle) => void;
}) {
  useEffect(() => {
    const timer = setTimeout(
      () =>
        onHandle({
          setValue: () => {},
          focus: () => calls.push("focus"),
          revealPosition: (line, column) => calls.push(`reveal ${line}:${column}`),
        }),
      50,
    );
    return () => clearTimeout(timer);
  }, [calls, onHandle]);
  return <p>loading the editor</p>;
}

describe("DocumentTabs", () => {
  it("shows the empty slot until a file is chosen in the explorer, then a named tab list", async () => {
    const { open } = setup({ "src/a.ts": textFile("src/a.ts", "one") });
    expect(screen.getByText("Nothing open")).toBeTruthy();

    await open("src/a.ts");
    const list = screen.getByRole("tablist", { name: "Open files" });
    expect(within(list).getByRole("tab", { name: "a.ts" }).getAttribute("aria-selected")).toBe(
      "true",
    );
    expect((screen.getByLabelText("Editor for src/a.ts") as HTMLTextAreaElement).value).toBe("one");
  });

  it("says a tab has unsaved changes in its name, and saves it from the registry", async () => {
    const { open, registry, written } = setup({ "a.ts": textFile("a.ts", "one") });
    await open("a.ts");
    expect(registry.isEnabled("file.save")).toBe(true);
    expect(registry.isEnabled("file.saveAll")).toBe(false);

    fireEvent.change(screen.getByLabelText("Editor for a.ts"), { target: { value: "two" } });
    expect(screen.getByRole("tab", { name: "a.ts, unsaved changes" })).toBeTruthy();
    expect(registry.isEnabled("file.saveAll")).toBe(true);

    await act(async () => {
      await registry.execute("file.save");
    });
    expect(written).toEqual(["a.ts:two"]);
    expect(screen.getByRole("tab", { name: "a.ts" })).toBeTruthy();
  });

  it("asks before closing a tab with unsaved changes, and closes it on Don't save", async () => {
    const { open } = setup({ "a.ts": textFile("a.ts", "one") });
    await open("a.ts");
    fireEvent.change(screen.getByLabelText("Editor for a.ts"), { target: { value: "two" } });

    fireEvent.keyDown(screen.getByRole("tab", { name: "a.ts, unsaved changes" }), {
      key: "Delete",
    });
    const dialog = await screen.findByRole("alertdialog", { name: "Save changes to a.ts?" });
    // Nothing is lost by pressing Enter without reading: focus starts on Cancel.
    expect(document.activeElement?.textContent).toBe("Cancel");

    fireEvent.click(within(dialog).getByRole("button", { name: "Don't save" }));
    await waitFor(() => expect(screen.getByText("Nothing open")).toBeTruthy());
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("closes a clean tab with Delete without asking", async () => {
    const { open } = setup({ "a.ts": textFile("a.ts", "one"), "b.ts": textFile("b.ts", "b") });
    await open("a.ts");
    await open("b.ts");
    fireEvent.keyDown(screen.getByRole("tab", { name: "b.ts" }), { key: "Delete" });
    await waitFor(() => expect(screen.queryByRole("tab", { name: "b.ts" })).toBeNull());
    expect(screen.getByRole("tab", { name: "a.ts" }).getAttribute("aria-selected")).toBe("true");
  });

  it("labels two files with the same name by their folders", async () => {
    const { open } = setup({
      "src/index.ts": textFile("src/index.ts", "a"),
      "test/index.ts": textFile("test/index.ts", "b"),
    });
    await open("src/index.ts");
    await open("test/index.ts");
    expect(screen.getByRole("tab", { name: "index.ts — src" })).toBeTruthy();
    expect(screen.getByRole("tab", { name: "index.ts — test" })).toBeTruthy();
  });

  it("explains a file it can't show, and a read-only one", async () => {
    const { open } = setup({
      "logo.png": { kind: "binary", path: "logo.png", stamp } as ReadFileDto,
      "big.json": textFile("big.json", "{}", { readOnly: "tooLargeToEdit" }),
    });
    await open("logo.png");
    expect(screen.getByRole("heading", { name: "logo.png isn't text" })).toBeTruthy();

    await open("big.json");
    expect(
      screen.getByText("big.json is too large to edit here, so it opened read-only."),
    ).toBeTruthy();
    expect((screen.getByLabelText("Editor for big.json") as HTMLTextAreaElement).readOnly).toBe(
      true,
    );
  });

  it("moves to the next and previous tab, round from the end", async () => {
    const { open, registry, documents } = setup({
      "a.ts": textFile("a.ts", "a"),
      "b.ts": textFile("b.ts", "b"),
    });
    await open("a.ts");
    expect(registry.isEnabled("tabs.next")).toBe(false);
    await open("b.ts");
    const active = () =>
      documents.getState().tabs.find((tab) => tab.id === documents.getState().activeId)?.path;
    expect(active()).toBe("b.ts");
    await act(async () => {
      await registry.execute("tabs.next");
    });
    expect(active()).toBe("a.ts");
    await act(async () => {
      await registry.execute("tabs.previous");
    });
    expect(active()).toBe("b.ts");
  });

  it("registers its commands only while it's shown", () => {
    const { registry, unmount } = setup({});
    expect(registry.get("tabs.close")?.title).toBe("Close Editor");
    unmount();
    expect(registry.get("tabs.close")).toBeUndefined();
  });

  it("reloads a clean file changed on disk into the editor, and asks about one with unsaved changes", async () => {
    const files: Record<string, ReadFileDto> = { "a.ts": textFile("a.ts", "one") };
    const port: DocumentsPort = {
      readFile: async (path) =>
        files[path] ? ok(files[path]) : { ok: false, error: { code: "notFound", message: "gone" } },
      writeFile: async (_path, text) => ok({ size: text.length, modifiedMs: 2 }),
    };
    const documents = createDocuments(port);
    const registry = createCommandRegistry<AppCommandId>();
    const bus = createEventBus<AppEvents>();
    const setValues: string[] = [];
    render(
      <DocumentTabs
        documents={documents}
        registry={registry}
        bus={bus}
        empty={<p>Nothing open</p>}
        renderEditor={(props) => (
          <textarea
            key={props.documentId}
            aria-label={`Editor for ${props.path}`}
            defaultValue={props.initialValue}
            onChange={(event) => props.onChange(event.target.value)}
            ref={(element) =>
              props.ref(
                element && {
                  setValue: (value) => {
                    setValues.push(value);
                    element.value = value;
                  },
                  focus: () => element.focus(),
                  revealPosition: () => element.focus(),
                },
              )
            }
          />
        )}
      />,
    );
    await act(async () => {
      bus.emit("explorer.fileActivated", { path: "a.ts" });
    });
    const editor = (await screen.findByLabelText("Editor for a.ts")) as HTMLTextAreaElement;

    files["a.ts"] = textFile("a.ts", "two");
    await act(async () => {
      bus.emit("workspace.filesChanged", {
        changes: [{ kind: "modified", path: "a.ts" }],
        rescan: false,
      });
    });
    await waitFor(() => expect(editor.value).toBe("two"));
    expect(setValues).toEqual(["two"]);

    fireEvent.change(editor, { target: { value: "mine" } });
    files["a.ts"] = textFile("a.ts", "theirs");
    await act(async () => {
      bus.emit("workspace.filesChanged", {
        changes: [{ kind: "modified", path: "a.ts" }],
        rescan: false,
      });
    });
    const bar = await screen.findByRole("alert");
    expect(within(bar).getByText("a.ts changed on disk")).toBeTruthy();
    expect(
      screen.getByRole("tab", { name: "a.ts, unsaved changes, changed on disk" }),
    ).toBeTruthy();

    fireEvent.click(within(bar).getByRole("button", { name: "Reload from disk" }));
    await waitFor(() => expect(editor.value).toBe("theirs"));
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByRole("tab", { name: "a.ts" })).toBeTruthy();
  });

  it("opens a file asked for elsewhere, then puts the cursor where it was asked", async () => {
    const files: Record<string, ReadFileDto> = {
      "a.ts": textFile("a.ts", "one"),
      "b.ts": textFile("b.ts", "two"),
    };
    const port: DocumentsPort = {
      readFile: async (path) =>
        files[path] ? ok(files[path]) : { ok: false, error: { code: "notFound", message: "gone" } },
      writeFile: async (_path, text) => ok({ size: text.length, modifiedMs: 2 }),
    };
    const documents = createDocuments(port);
    const bus = createEventBus<AppEvents>();
    const calls: string[] = [];
    render(
      <DocumentTabs
        documents={documents}
        registry={createCommandRegistry<AppCommandId>()}
        bus={bus}
        empty={<p>Nothing open</p>}
        renderEditor={(props) => (
          <textarea
            aria-label={`Editor for ${props.path}`}
            defaultValue={props.initialValue}
            ref={(element) =>
              props.ref(
                element && {
                  setValue: () => {},
                  focus: () => calls.push(`focus ${props.path}`),
                  revealPosition: (line, column) =>
                    calls.push(`reveal ${props.path} ${line}:${column}`),
                },
              )
            }
          />
        )}
      />,
    );

    await act(async () => {
      bus.emit("file.openRequested", { path: "a.ts", line: 3, column: 7 });
    });
    await waitFor(() => expect(calls).toEqual(["reveal a.ts 3:7"]));

    // Without a position, the editor only takes focus.
    await act(async () => {
      bus.emit("file.openRequested", { path: "b.ts" });
    });
    await waitFor(() => expect(calls).toEqual(["reveal a.ts 3:7", "focus b.ts"]));

    // A file already open is shown again, at the new position.
    await act(async () => {
      bus.emit("file.openRequested", { path: "a.ts", line: 1, column: 1 });
    });
    await waitFor(() => expect(calls.at(-1)).toBe("reveal a.ts 1:1"));
    expect(documents.getState().tabs).toHaveLength(2);
  });

  it("waits for an editor whose handle comes late, as one that loads its code does", async () => {
    const port: DocumentsPort = {
      readFile: async (path) => ok(textFile(path, "text")),
      writeFile: async (_path, text) => ok({ size: text.length, modifiedMs: 2 }),
    };
    const documents = createDocuments(port);
    const bus = createEventBus<AppEvents>();
    const calls: string[] = [];
    render(
      <DocumentTabs
        documents={documents}
        registry={createCommandRegistry<AppCommandId>()}
        bus={bus}
        empty={<p>Nothing open</p>}
        renderEditor={(props) => <LateEditor calls={calls} onHandle={props.ref} />}
      />,
    );
    await act(async () => {
      bus.emit("file.openRequested", { path: "a.ts", line: 4, column: 2 });
    });
    await waitFor(() => expect(calls).toEqual(["reveal 4:2"]));
  });

  it("tells the app which files are open and their text, as it changes", async () => {
    const { open, bus, registry } = setup({ "a.ts": textFile("a.ts", "one") });
    const events: string[] = [];
    bus.on("document.opened", ({ path, text }) => events.push(`opened ${path} ${text}`));
    bus.on("document.changed", ({ path, text }) => events.push(`changed ${path} ${text}`));
    bus.on("document.closed", ({ path }) => events.push(`closed ${path}`));

    await open("a.ts");
    expect(events).toEqual(["opened a.ts one"]);
    fireEvent.change(screen.getByLabelText("Editor for a.ts"), { target: { value: "two" } });
    expect(events.at(-1)).toBe("changed a.ts two");
    // Unsaved: it asks first (the command waits for the answer); don't save, and it closes.
    act(() => {
      void registry.execute("tabs.close");
    });
    fireEvent.click(await screen.findByRole("button", { name: "Don't save" }));
    await waitFor(() => expect(events.at(-1)).toBe("closed a.ts"));
  });
});
