import type {
  DocumentsPort,
  FileStampDto,
  IpcError,
  IpcResult,
  ReadFileDto,
} from "@repo/contracts";
import { describe, expect, it, vi } from "vitest";
import { createDocuments, type DocumentId } from "./documents";
import { endingAfterSave, toDiskText, toEditorText } from "./line-endings";

const stamp: FileStampDto = { size: 10, modifiedMs: 1 };
const ok = <T>(value: T): IpcResult<T> => ({ ok: true, value });
const fail = (code: IpcError["code"], message: string = code): IpcResult<never> => ({
  ok: false,
  error: { code, message },
});
const text = (path: string, body: string, extra: Partial<ReadFileDto> = {}): ReadFileDto =>
  ({
    kind: "text",
    path,
    text: body,
    stamp,
    lineEnding: "lf",
    readOnly: null,
    ...extra,
  }) as ReadFileDto;

/** A workspace of files in memory. `written` records each write. */
function fakePort(files: Record<string, ReadFileDto | IpcError>) {
  const written: { path: string; text: string }[] = [];
  let failWrites = new Set<string>();
  const port: DocumentsPort = {
    readFile: async (path) => {
      const file = files[path];
      if (!file) return fail("notFound");
      return "code" in file ? { ok: false, error: file } : ok(file);
    },
    writeFile: async (path, body) => {
      if (failWrites.has(path)) return fail("permissionDenied", "Access is denied.");
      written.push({ path, text: body });
      return ok({ size: body.length, modifiedMs: 2 });
    },
  };
  return {
    port,
    written,
    failWritesTo: (...paths: string[]) => {
      failWrites = new Set(paths);
    },
  };
}

const ids = (docs: ReturnType<typeof createDocuments>) =>
  docs.getState().tabs.map((tab) => tab.path);

async function openAll(docs: ReturnType<typeof createDocuments>, ...paths: string[]) {
  const opened: DocumentId[] = [];
  for (const path of paths) {
    const id = await docs.open(path);
    if (!id) throw new Error(`could not open ${path}`);
    opened.push(id);
  }
  return opened;
}

describe("line endings", () => {
  it.each([
    ["a\r\nb\r\n", "a\nb\n"],
    ["a\rb", "a\nb"],
    ["a\nb\r\nc", "a\nb\nc"],
    ["one line", "one line"],
  ])("toEditorText(%j) is %j", (input, expected) => {
    expect(toEditorText(input)).toBe(expected);
  });

  it("puts CRLF back only for a CRLF file", () => {
    expect(toDiskText("a\nb\n", "crlf")).toBe("a\r\nb\r\n");
    expect(toDiskText("a\nb\n", "lf")).toBe("a\nb\n");
    expect(toDiskText("a\nb\n", "mixed")).toBe("a\nb\n");
  });

  it.each([
    ["crlf", "a", "crlf"],
    ["lf", "a", "lf"],
    ["mixed", "a\nb", "lf"],
    ["none", "a", "none"],
    ["none", "a\nb", "lf"],
  ] as const)("a %s file saved as %j is %s", (before, saved, after) => {
    expect(endingAfterSave(before, saved)).toBe(after);
  });
});

describe("opening", () => {
  it("opens a file in a new, active tab next to the active one", async () => {
    const { port } = fakePort({
      "a.ts": text("a.ts", "a"),
      "b.ts": text("b.ts", "b"),
      "src/c.ts": text("src/c.ts", "c"),
    });
    const docs = createDocuments(port);
    const [a] = await openAll(docs, "a.ts", "b.ts");
    docs.activate(a as DocumentId);
    const c = await docs.open("src/c.ts");

    expect(ids(docs)).toEqual(["a.ts", "src/c.ts", "b.ts"]);
    expect(docs.getState().activeId).toBe(c);
    expect(docs.getState().tabs[1]).toMatchObject({ name: "c.ts", dirty: false });
  });

  it("activates the tab a file is already open in", async () => {
    const { port } = fakePort({ "a.ts": text("a.ts", "a"), "b.ts": text("b.ts", "b") });
    const docs = createDocuments(port);
    const [a] = await openAll(docs, "a.ts", "b.ts");

    expect(await docs.open("a.ts")).toBe(a);
    expect(ids(docs)).toEqual(["a.ts", "b.ts"]);
    expect(docs.getState().activeId).toBe(a);
  });

  it("shows the tab while the file is read", async () => {
    const { port } = fakePort({ "a.ts": text("a.ts", "a") });
    const docs = createDocuments(port);
    const opening = docs.open("a.ts");
    expect(docs.getState().tabs[0]?.content).toEqual({ kind: "loading" });
    await opening;
    expect(docs.getState().tabs[0]?.content.kind).toBe("text");
  });

  it("keeps the editor's form of the text, and the file's line ending", async () => {
    const { port } = fakePort({ "a.ts": text("a.ts", "x\r\ny\r\n", { lineEnding: "crlf" }) });
    const docs = createDocuments(port);
    const [a] = await openAll(docs, "a.ts");
    expect(docs.getText(a as DocumentId)).toBe("x\ny\n");
    expect(docs.getState().tabs[0]?.content).toMatchObject({ kind: "text", lineEnding: "crlf" });
  });

  it.each([
    [{ kind: "tooLarge", path: "big.log", stamp, limit: 5 } as ReadFileDto, "tooLarge"],
    [{ kind: "binary", path: "big.log", stamp } as ReadFileDto, "binary"],
    [{ kind: "notUtf8", path: "big.log", stamp } as ReadFileDto, "notUtf8"],
  ])("opens a file that can't be shown as a notice (%#)", async (file, kind) => {
    const { port } = fakePort({ "big.log": file });
    const docs = createDocuments(port);
    await docs.open("big.log");
    expect(docs.getState().tabs[0]?.content.kind).toBe(kind);
  });

  it("keeps a read-only reason", async () => {
    const { port } = fakePort({ "a.ts": text("a.ts", "a", { readOnly: "tooLargeToEdit" }) });
    const docs = createDocuments(port);
    await docs.open("a.ts");
    expect(docs.getState().tabs[0]?.content).toMatchObject({ readOnly: "tooLargeToEdit" });
  });

  it("removes the tab and reports a file that can't be read", async () => {
    const onError = vi.fn();
    const { port } = fakePort({});
    const docs = createDocuments(port, { onError });

    expect(await docs.open("gone.ts")).toBeNull();
    expect(docs.getState()).toMatchObject({ tabs: [], activeId: null });
    expect(onError).toHaveBeenCalledWith(
      "Couldn't open gone.ts",
      expect.objectContaining({ code: "notFound" }),
    );
  });
});

describe("tabs", () => {
  it("reorders, clamping the index", async () => {
    const { port } = fakePort({
      "a.ts": text("a.ts", "a"),
      "b.ts": text("b.ts", "b"),
      "c.ts": text("c.ts", "c"),
    });
    const docs = createDocuments(port);
    const [a, , c] = await openAll(docs, "a.ts", "b.ts", "c.ts");

    docs.reorder(a as DocumentId, 2);
    expect(ids(docs)).toEqual(["b.ts", "c.ts", "a.ts"]);
    docs.reorder(c as DocumentId, -5);
    expect(ids(docs)).toEqual(["c.ts", "b.ts", "a.ts"]);
    docs.reorder(c as DocumentId, 99);
    expect(ids(docs)).toEqual(["b.ts", "a.ts", "c.ts"]);
  });

  it.each([
    ["the middle tab, to its right neighbour", 1, "c.ts"],
    ["the last tab, to its left neighbour", 2, "b.ts"],
  ])("closing %s", async (_, index, next) => {
    const { port } = fakePort({
      "a.ts": text("a.ts", "a"),
      "b.ts": text("b.ts", "b"),
      "c.ts": text("c.ts", "c"),
    });
    const docs = createDocuments(port);
    const opened = await openAll(docs, "a.ts", "b.ts", "c.ts");
    const closing = opened[index] as DocumentId;
    docs.activate(closing);

    expect(await docs.requestClose([closing])).toBe(true);
    const active = docs.getState().tabs.find((tab) => tab.id === docs.getState().activeId);
    expect(active?.path).toBe(next);
  });

  it("closing an inactive tab keeps the active one", async () => {
    const { port } = fakePort({ "a.ts": text("a.ts", "a"), "b.ts": text("b.ts", "b") });
    const docs = createDocuments(port);
    const [a, b] = await openAll(docs, "a.ts", "b.ts");
    await docs.requestClose([a as DocumentId]);
    expect(docs.getState()).toMatchObject({ activeId: b });
    expect(docs.getText(a as DocumentId)).toBeUndefined();
  });

  it("closing the last tab leaves none active", async () => {
    const { port } = fakePort({ "a.ts": text("a.ts", "a") });
    const docs = createDocuments(port);
    const [a] = await openAll(docs, "a.ts");
    await docs.requestClose([a as DocumentId]);
    expect(docs.getState()).toMatchObject({ tabs: [], activeId: null });
  });
});

describe("dirty state and saving", () => {
  it("is dirty while the text differs from the saved text, and only notifies on a change of state", async () => {
    const { port } = fakePort({ "a.ts": text("a.ts", "hello") });
    const docs = createDocuments(port);
    const [a] = await openAll(docs, "a.ts");
    const id = a as DocumentId;
    const listener = vi.fn();
    docs.subscribe(listener);

    docs.setText(id, "hello!");
    docs.setText(id, "hello!!");
    expect(docs.getState().tabs[0]?.dirty).toBe(true);
    expect(listener).toHaveBeenCalledTimes(1);

    docs.setText(id, "hello"); // typed back to what's on disk
    expect(docs.getState().tabs[0]?.dirty).toBe(false);
    expect(docs.hasUnsaved()).toBe(false);
  });

  it("saves with the file's line ending, and is clean afterwards", async () => {
    const { port, written } = fakePort({ "a.ts": text("a.ts", "x\r\n", { lineEnding: "crlf" }) });
    const docs = createDocuments(port);
    const [a] = await openAll(docs, "a.ts");
    const id = a as DocumentId;
    docs.setText(id, "x\ny\n");

    expect(await docs.save(id)).toBe(true);
    expect(written).toEqual([{ path: "a.ts", text: "x\r\ny\r\n" }]);
    expect(docs.getState().tabs[0]).toMatchObject({
      dirty: false,
      saving: false,
      content: { stamp: { size: 6, modifiedMs: 2 } },
    });
  });

  it("writes a file with mixed line endings with LF, and says so afterwards", async () => {
    const { port, written } = fakePort({
      "a.ts": text("a.ts", "x\r\ny\n", { lineEnding: "mixed" }),
    });
    const docs = createDocuments(port);
    const [a] = await openAll(docs, "a.ts");
    docs.setText(a as DocumentId, "x\ny\nz\n");
    await docs.save(a as DocumentId);
    expect(written[0]?.text).toBe("x\ny\nz\n");
    expect(docs.getState().tabs[0]?.content).toMatchObject({ lineEnding: "lf" });
  });

  it("doesn't write a clean document, or a read-only one", async () => {
    const { port, written } = fakePort({
      "a.ts": text("a.ts", "a"),
      "ro.ts": text("ro.ts", "r", { readOnly: "readOnlyOnDisk" }),
    });
    const docs = createDocuments(port);
    const [a, ro] = await openAll(docs, "a.ts", "ro.ts");
    expect(await docs.save(a as DocumentId)).toBe(true);
    docs.setText(ro as DocumentId, "changed");
    expect(await docs.save(ro as DocumentId)).toBe(false);
    expect(written).toEqual([]);
  });

  it("stays dirty when the text changes while it's saved", async () => {
    let finish: (value: IpcResult<FileStampDto>) => void = () => {};
    const docs = createDocuments({
      readFile: async () => ok(text("a.ts", "a")),
      writeFile: () => new Promise((resolve) => (finish = resolve)),
    });
    const [a] = await openAll(docs, "a.ts");
    const id = a as DocumentId;
    docs.setText(id, "ab");
    const saving = docs.save(id);
    expect(docs.getState().tabs[0]?.saving).toBe(true);
    docs.setText(id, "abc");
    finish(ok(stamp));
    await saving;
    expect(docs.getState().tabs[0]).toMatchObject({ dirty: true, saving: false });
  });

  it("reports a failed save and stays dirty", async () => {
    const onError = vi.fn();
    const { port, failWritesTo } = fakePort({ "a.ts": text("a.ts", "a") });
    const docs = createDocuments(port, { onError });
    const [a] = await openAll(docs, "a.ts");
    docs.setText(a as DocumentId, "b");
    failWritesTo("a.ts");

    expect(await docs.save(a as DocumentId)).toBe(false);
    expect(docs.getState().tabs[0]).toMatchObject({ dirty: true, saving: false });
    expect(onError).toHaveBeenCalledWith(
      "Couldn't save a.ts",
      expect.objectContaining({ code: "permissionDenied" }),
    );
  });

  it("saves every dirty document", async () => {
    const { port, written } = fakePort({
      "a.ts": text("a.ts", "a"),
      "b.ts": text("b.ts", "b"),
      "c.ts": text("c.ts", "c"),
    });
    const docs = createDocuments(port);
    const [a, , c] = await openAll(docs, "a.ts", "b.ts", "c.ts");
    docs.setText(a as DocumentId, "A");
    docs.setText(c as DocumentId, "C");
    expect(await docs.saveAll()).toBe(true);
    expect(written.map((write) => write.path).sort()).toEqual(["a.ts", "c.ts"]);
    expect(docs.hasUnsaved()).toBe(false);
  });
});

describe("closing documents with unsaved changes", () => {
  async function setup() {
    const port = fakePort({
      "a.ts": text("a.ts", "a"),
      "b.ts": text("b.ts", "b"),
      "c.ts": text("c.ts", "c"),
    });
    const docs = createDocuments(port.port);
    const [a, b, c] = (await openAll(docs, "a.ts", "b.ts", "c.ts")) as [
      DocumentId,
      DocumentId,
      DocumentId,
    ];
    docs.setText(a, "A");
    docs.setText(c, "C");
    return { ...port, docs, a, b, c };
  }

  it("asks about the dirty ones only, and closes nothing yet", async () => {
    const { docs, a, b, c } = await setup();
    void docs.requestClose([a, b, c]);
    expect(docs.getState().pendingClose?.dirty.map((tab) => tab.path)).toEqual(["a.ts", "c.ts"]);
    expect(docs.getState().tabs).toHaveLength(3);
  });

  it("cancel keeps everything open", async () => {
    const { docs, a, b } = await setup();
    const closing = docs.requestClose([a, b]);
    await docs.answerClose("cancel");
    expect(await closing).toBe(false);
    expect(docs.getState()).toMatchObject({ pendingClose: null });
    expect(docs.getState().tabs).toHaveLength(3);
  });

  it("don't save closes them and writes nothing", async () => {
    const { docs, written, a, b } = await setup();
    const closing = docs.requestClose([a, b]);
    await docs.answerClose("discard");
    expect(await closing).toBe(true);
    expect(ids(docs)).toEqual(["c.ts"]);
    expect(written).toEqual([]);
  });

  it("save writes the dirty ones, then closes them all", async () => {
    const { docs, written, a, b, c } = await setup();
    const closing = docs.requestClose([a, b, c]);
    await docs.answerClose("save");
    expect(await closing).toBe(true);
    expect(written.map((write) => write.path).sort()).toEqual(["a.ts", "c.ts"]);
    expect(docs.getState()).toMatchObject({ tabs: [], pendingClose: null });
  });

  it("keeps a document open when its save fails, and closes the rest", async () => {
    const { docs, failWritesTo, a, b, c } = await setup();
    failWritesTo("c.ts");
    const closing = docs.requestClose([a, b, c]);
    await docs.answerClose("save");
    expect(await closing).toBe(false);
    expect(ids(docs)).toEqual(["c.ts"]);
    expect(docs.getState()).toMatchObject({ activeId: c, pendingClose: null });
  });

  it("refuses a second request while one is open", async () => {
    const { docs, a, c } = await setup();
    void docs.requestClose([a]);
    expect(await docs.requestClose([c])).toBe(false);
    expect(docs.getState().pendingClose?.dirty.map((tab) => tab.path)).toEqual(["a.ts"]);
  });

  it("closes clean documents without asking", async () => {
    const { docs, b } = await setup();
    expect(await docs.requestClose([b])).toBe(true);
    expect(docs.getState().pendingClose).toBeNull();
  });
});

/** A disk in memory: writes land in `files`, so reading after a save sees them. */
function disk(initial: Record<string, string>, ending: "lf" | "crlf" = "lf") {
  const files = new Map(Object.entries(initial));
  let reads = 0;
  let readGate: Promise<void> | null = null;
  const port: DocumentsPort = {
    readFile: async (path) => {
      reads += 1;
      if (readGate) await readGate;
      const body = files.get(path);
      return body === undefined ? fail("notFound") : ok(text(path, body, { lineEnding: ending }));
    },
    writeFile: async (path, body) => {
      files.set(path, body);
      return ok({ size: body.length, modifiedMs: 3 });
    },
  };
  return {
    port,
    files,
    reads: () => reads,
    /** Holds reads until the returned function is called. */
    holdReads: () => {
      let release = () => {};
      readGate = new Promise((resolve) => {
        release = () => {
          readGate = null;
          resolve();
        };
      });
      return release;
    },
  };
}

describe("changes on disk", () => {
  it("takes the new text into a clean document, and tells the editor by revision", async () => {
    const fs = disk({ "a.ts": "one" });
    const docs = createDocuments(fs.port);
    const [a] = await openAll(docs, "a.ts");
    if (!a) throw new Error("not opened");
    const before = docs.getState().tabs[0]?.revision ?? 0;

    fs.files.set("a.ts", "two");
    await docs.checkDisk(["a.ts"]);
    expect(docs.getText(a)).toBe("two");
    expect(docs.getState().tabs[0]).toMatchObject({ dirty: false, disk: "same" });
    expect(docs.getState().tabs[0]?.revision).toBe(before + 1);
  });

  it("doesn't count its own save as a change", async () => {
    const fs = disk({ "a.ts": "a\r\nb" }, "crlf");
    const docs = createDocuments(fs.port);
    const [a] = await openAll(docs, "a.ts");
    if (!a) throw new Error("not opened");
    docs.setText(a, "a\nb\nc");
    await docs.save(a);
    const revision = docs.getState().tabs[0]?.revision;

    await docs.checkDisk(["a.ts"]);
    expect(docs.getState().tabs[0]).toMatchObject({ dirty: false, disk: "same", revision });
    expect(fs.files.get("a.ts")).toBe("a\r\nb\r\nc");
  });

  it("asks about a document with unsaved changes: reload or keep mine", async () => {
    const fs = disk({ "a.ts": "one" });
    const docs = createDocuments(fs.port);
    const [a] = await openAll(docs, "a.ts");
    if (!a) throw new Error("not opened");
    docs.setText(a, "mine");

    fs.files.set("a.ts", "theirs");
    await docs.checkDisk(["a.ts"]);
    expect(docs.getState().tabs[0]).toMatchObject({ dirty: true, disk: "changed" });
    expect(docs.getText(a)).toBe("mine");

    docs.keepMine(a);
    expect(docs.getState().tabs[0]?.disk).toBe("same");
    await docs.save(a);
    expect(fs.files.get("a.ts")).toBe("mine");

    // And the other answer.
    docs.setText(a, "mine again");
    fs.files.set("a.ts", "theirs again");
    await docs.checkDisk(["a.ts"]);
    expect(await docs.reload(a)).toBe(true);
    expect(docs.getText(a)).toBe("theirs again");
    expect(docs.getState().tabs[0]).toMatchObject({ dirty: false, disk: "same" });
  });

  it("settles by itself when the file comes to say what the editor does", async () => {
    const fs = disk({ "a.ts": "one" });
    const docs = createDocuments(fs.port);
    const [a] = await openAll(docs, "a.ts");
    if (!a) throw new Error("not opened");
    docs.setText(a, "two");
    fs.files.set("a.ts", "two");
    await docs.checkDisk(["a.ts"]);
    expect(docs.getState().tabs[0]).toMatchObject({ dirty: false, disk: "same" });
  });

  it("marks a deleted file, keeps its text, and saving brings it back", async () => {
    const fs = disk({ "src/a.ts": "one" });
    const docs = createDocuments(fs.port);
    const [a] = await openAll(docs, "src/a.ts");
    if (!a) throw new Error("not opened");

    // The folder went: its files are checked too.
    fs.files.delete("src/a.ts");
    await docs.checkDisk(["src"]);
    expect(docs.getState().tabs[0]).toMatchObject({ dirty: false, disk: "deleted" });
    expect(docs.getText(a)).toBe("one");

    expect(await docs.save(a)).toBe(true);
    expect(fs.files.get("src/a.ts")).toBe("one");
    expect(docs.getState().tabs[0]?.disk).toBe("same");
  });

  it("checks only the documents at or under the paths given, or all for null", async () => {
    const fs = disk({ "a.ts": "a", "src/b.ts": "b", "srcx.ts": "x" });
    const docs = createDocuments(fs.port);
    await openAll(docs, "a.ts", "src/b.ts", "srcx.ts");
    const opened = fs.reads();
    await docs.checkDisk(["src"]);
    expect(fs.reads()).toBe(opened + 1);
    await docs.checkDisk(null);
    expect(fs.reads()).toBe(opened + 4);
  });

  it("looks again after a save that was running when the change came in", async () => {
    const fs = disk({ "a.ts": "one" });
    const docs = createDocuments(fs.port);
    const [a] = await openAll(docs, "a.ts");
    if (!a) throw new Error("not opened");
    docs.setText(a, "two");
    const saving = docs.save(a);
    // The watcher reports the save before the save has resolved.
    await docs.checkDisk(["a.ts"]);
    await saving;
    await vi.waitFor(() => expect(docs.getState().tabs[0]?.saving).toBe(false));
    await Promise.resolve();
    expect(docs.getState().tabs[0]).toMatchObject({ dirty: false, disk: "same" });
    expect(docs.getText(a)).toBe("two");
  });

  it("checks again when a change comes in while it's still reading the last one", async () => {
    const fs = disk({ "a.ts": "one" });
    const docs = createDocuments(fs.port);
    const [a] = await openAll(docs, "a.ts");
    if (!a) throw new Error("not opened");
    const release = fs.holdReads();
    const first = docs.checkDisk(["a.ts"]);
    fs.files.set("a.ts", "two");
    await docs.checkDisk(["a.ts"]);
    release();
    await first;
    await vi.waitFor(() => expect(docs.getText(a)).toBe("two"));
  });
});
