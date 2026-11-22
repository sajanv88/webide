// The open documents of one workspace, as plain TypeScript: which are open, in
// what order, which is active, which have unsaved changes, and the question
// asked before closing one that does. React reads it with useSyncExternalStore.
import type {
  DocumentsPort,
  FileStampDto,
  IpcError,
  LineEndingDto,
  ReadFileDto,
  ReadOnlyReasonDto,
} from "@repo/contracts";
import { endingAfterSave, toDiskText, toEditorText } from "./line-endings";

declare const documentIdBrand: unique symbol;
/** One open document. A file closed and opened again gets a new id. */
export type DocumentId = string & { readonly [documentIdBrand]: "DocumentId" };

/** What a tab shows. Switch on `kind`. */
export type DocumentContent =
  | { readonly kind: "loading" }
  | {
      readonly kind: "text";
      readonly lineEnding: LineEndingDto;
      /** Set when it can't be edited, and why. */
      readonly readOnly: ReadOnlyReasonDto | null;
      readonly stamp: FileStampDto;
    }
  /** Over the "refuse above" limit: not read at all. */
  | { readonly kind: "tooLarge"; readonly size: number; readonly limit: number }
  | { readonly kind: "binary" | "notUtf8"; readonly size: number };

/**
 * The file on disk, compared with the document. `changed`: it changed while
 * the document had unsaved changes, so the person chooses (`reload` or
 * `keepMine`). `deleted`: it's gone; saving creates it again.
 */
export type DiskState = "same" | "changed" | "deleted";

export interface DocumentView {
  readonly id: DocumentId;
  /** Workspace-relative, `/`-separated. */
  readonly path: string;
  /** The file's name: the last segment of `path`. */
  readonly name: string;
  readonly content: DocumentContent;
  /** The text differs from what was last read or saved. */
  readonly dirty: boolean;
  readonly saving: boolean;
  readonly disk: DiskState;
  /**
   * Increases each time the text is replaced from disk. An editor that showed
   * an older revision must take the new text (`getText`).
   */
  readonly revision: number;
}

/** The question asked before closing documents with unsaved changes. */
export interface PendingClose {
  /** The documents asked about: the ones with unsaved changes. */
  readonly dirty: readonly DocumentView[];
  /** "Save" was chosen and the saves are running. */
  readonly saving: boolean;
}

export interface DocumentsState {
  /** In tab order. */
  readonly tabs: readonly DocumentView[];
  /** `null` only when no document is open. */
  readonly activeId: DocumentId | null;
  readonly pendingClose: PendingClose | null;
}

export type CloseChoice = "save" | "discard" | "cancel";

export interface Documents {
  getState(): DocumentsState;
  /** Called after every change to the state. Returns the unsubscribe. */
  subscribe(listener: () => void): () => void;
  /** Opens a file in a tab, or activates the tab it's already in. `null` when it couldn't be read. */
  open(path: string): Promise<DocumentId | null>;
  activate(id: DocumentId): void;
  /** Moves a tab to `toIndex` (clamped to the tabs there are). */
  reorder(id: DocumentId, toIndex: number): void;
  /** The text as the editor holds it now (`\n` line breaks). */
  getText(id: DocumentId): string | undefined;
  /** What the editor reports on each change. */
  setText(id: DocumentId, text: string): void;
  /** Writes a text document. Resolves to whether it's saved now (a clean one already is). */
  save(id: DocumentId): Promise<boolean>;
  saveAll(): Promise<boolean>;
  /**
   * Closes documents, asking first about any with unsaved changes (through
   * `pendingClose`, answered with `answerClose`). Resolves to whether all of
   * them closed. While a question is open, another request resolves to `false`.
   */
  requestClose(ids: readonly DocumentId[]): Promise<boolean>;
  answerClose(choice: CloseChoice): Promise<void>;
  hasUnsaved(): boolean;
  /**
   * Files changed on disk: compares each open document at or under `paths`
   * (every one, for `null`) with its file. A clean one takes the new text; one
   * with unsaved changes is marked `changed`; one whose file is gone, `deleted`.
   * The document's own saves change nothing.
   */
  checkDisk(paths: readonly string[] | null): Promise<void>;
  /** Replaces the text with the file's, dropping unsaved changes. Resolves to whether it could. */
  reload(id: DocumentId): Promise<boolean>;
  /** Keeps the unsaved text over a file that changed on disk; saving overwrites it. */
  keepMine(id: DocumentId): void;
}

export interface DocumentsOptions {
  /** A file couldn't be opened or saved. `title` names the file. */
  readonly onError?: (title: string, error: IpcError) => void;
}

interface Texts {
  /** What was last read or saved, as the editor holds it. */
  saved: string;
  current: string;
}

const nameOf = (path: string): string => path.slice(path.lastIndexOf("/") + 1) || path;

/** `path` is `root` or inside it. */
const within = (path: string, root: string): boolean =>
  root === "" || path === root || path.startsWith(`${root}/`);

/** A disk check per document: one at a time, and one more if asked meanwhile. */
interface Check {
  busy: boolean;
  again: boolean;
}

let lastId = 0;

export function createDocuments(port: DocumentsPort, options: DocumentsOptions = {}): Documents {
  let state: DocumentsState = { tabs: [], activeId: null, pendingClose: null };
  const texts = new Map<DocumentId, Texts>();
  const checks = new Map<DocumentId, Check>();
  const listeners = new Set<() => void>();
  let answer: ((choice: CloseChoice) => Promise<void>) | null = null;

  const set = (next: Partial<DocumentsState>) => {
    state = { ...state, ...next };
    for (const listener of [...listeners]) listener();
  };
  const find = (id: DocumentId) => state.tabs.find((tab) => tab.id === id);
  const update = (id: DocumentId, change: Partial<DocumentView>) => {
    if (!find(id)) return;
    set({ tabs: state.tabs.map((tab) => (tab.id === id ? { ...tab, ...change } : tab)) });
  };

  /** Removes tabs at once, and activates the neighbour of an active one that goes. */
  const closeNow = (ids: ReadonlySet<DocumentId>) => {
    if (ids.size === 0) return;
    const before = state.tabs;
    const tabs = before.filter((tab) => !ids.has(tab.id));
    let activeId = state.activeId;
    if (activeId !== null && ids.has(activeId)) {
      const at = before.findIndex((tab) => tab.id === activeId);
      const after = before.slice(at + 1).find((tab) => !ids.has(tab.id));
      const beforeIt = before
        .slice(0, at)
        .reverse()
        .find((tab) => !ids.has(tab.id));
      activeId = (after ?? beforeIt)?.id ?? null;
    }
    for (const id of ids) {
      texts.delete(id);
      checks.delete(id);
    }
    set({ tabs, activeId });
  };

  /**
   * Shows what was read from disk, as the document's content and its saved
   * text. Unsaved changes are dropped; the editor is told by `revision`.
   */
  const adopt = (id: DocumentId, file: ReadFileDto) => {
    const tab = find(id);
    if (!tab) return;
    const base = { dirty: false, disk: "same" as const, revision: tab.revision + 1 };
    switch (file.kind) {
      case "text": {
        const text = toEditorText(file.text);
        texts.set(id, { saved: text, current: text });
        update(id, {
          ...base,
          content: {
            kind: "text",
            lineEnding: file.lineEnding,
            readOnly: file.readOnly,
            stamp: file.stamp,
          },
        });
        break;
      }
      case "tooLarge":
        texts.delete(id);
        update(id, {
          ...base,
          content: { kind: "tooLarge", size: file.stamp.size, limit: file.limit },
        });
        break;
      case "binary":
      case "notUtf8":
        texts.delete(id);
        update(id, { ...base, content: { kind: file.kind, size: file.stamp.size } });
        break;
    }
  };

  /** Runs a check asked for while the document was busy (being read, saved or checked). */
  const settle = (id: DocumentId) => {
    const pending = checks.get(id);
    if (pending?.again && !pending.busy) {
      pending.again = false;
      void check(id);
    }
  };

  /** Compares one open document with its file on disk. */
  const check = async (id: DocumentId): Promise<void> => {
    const tab = find(id);
    if (!tab) return;
    let pending = checks.get(id);
    if (!pending) {
      pending = { busy: false, again: false };
      checks.set(id, pending);
    }
    // Mid-read or mid-save, the disk is about to be ours: look once that's done.
    if (pending.busy || tab.saving || tab.content.kind === "loading") {
      pending.again = true;
      return;
    }
    pending.busy = true;
    const result = await port.readFile(tab.path);
    pending.busy = false;
    const now = find(id);
    if (!now) return;
    if (pending.again || now.saving) {
      // What was read may already be out of date: look again once settled.
      pending.again = true;
      if (!now.saving) settle(id);
      return;
    }
    if (!result.ok) {
      if (result.error.code === "notFound") update(id, { disk: "deleted" });
      return;
    }
    const file = result.value;
    const entry = texts.get(id);
    if (file.kind === "text" && now.content.kind === "text" && entry) {
      const text = toEditorText(file.text);
      const content = {
        ...now.content,
        lineEnding: file.lineEnding,
        readOnly: file.readOnly,
        stamp: file.stamp,
      };
      if (text === entry.saved) {
        // Our own save, or a change to the file that left its text as it was.
        update(id, { content, disk: "same" });
      } else if (!now.dirty) {
        adopt(id, file);
      } else if (text === entry.current) {
        // The file now says what the editor does.
        entry.saved = text;
        update(id, { content, disk: "same", dirty: false });
      } else {
        update(id, { disk: "changed" });
      }
      return;
    }
    // It became text, or stopped being text, or still isn't text.
    if (now.dirty) update(id, { disk: "changed" });
    else adopt(id, file);
  };

  const save = async (id: DocumentId): Promise<boolean> => {
    const tab = find(id);
    const entry = texts.get(id);
    if (!tab || !entry || tab.content.kind !== "text" || tab.content.readOnly) return false;
    // A deleted file is written even without changes: saving brings it back.
    if (!tab.dirty && tab.disk !== "deleted") return true;
    if (tab.saving) return false;
    const text = entry.current;
    const { lineEnding } = tab.content;
    update(id, { saving: true });
    const result = await port.writeFile(tab.path, toDiskText(text, lineEnding));
    const now = find(id);
    if (now?.content.kind !== "text") return result.ok;
    if (!result.ok) {
      update(id, { saving: false });
      settle(id);
      options.onError?.(`Couldn't save ${tab.name}`, result.error);
      return false;
    }
    entry.saved = text;
    update(id, {
      saving: false,
      dirty: entry.current !== entry.saved,
      // What's on disk is now this document's text.
      disk: "same",
      content: {
        ...now.content,
        lineEnding: endingAfterSave(lineEnding, text),
        stamp: result.value,
      },
    });
    settle(id);
    return true;
  };

  return {
    getState: () => state,

    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },

    async open(path) {
      const existing = state.tabs.find((tab) => tab.path === path);
      if (existing) {
        set({ activeId: existing.id });
        return existing.id;
      }
      lastId += 1;
      const id = `doc-${lastId}` as DocumentId;
      const tab: DocumentView = {
        id,
        path,
        name: nameOf(path),
        content: { kind: "loading" },
        dirty: false,
        saving: false,
        disk: "same",
        revision: 0,
      };
      // A new tab opens next to the active one.
      const at = state.tabs.findIndex((other) => other.id === state.activeId);
      const tabs = [...state.tabs];
      tabs.splice(at + 1, 0, tab);
      set({ tabs, activeId: id });

      const result = await port.readFile(path);
      if (!find(id)) return null; // closed while it was being read
      if (!result.ok) {
        closeNow(new Set([id]));
        options.onError?.(`Couldn't open ${tab.name}`, result.error);
        return null;
      }
      adopt(id, result.value);
      settle(id);
      return id;
    },

    activate(id) {
      if (find(id) && state.activeId !== id) set({ activeId: id });
    },

    reorder(id, toIndex) {
      const from = state.tabs.findIndex((tab) => tab.id === id);
      if (from === -1) return;
      const to = Math.max(0, Math.min(state.tabs.length - 1, Math.trunc(toIndex)));
      if (from === to) return;
      const tabs = [...state.tabs];
      const [moved] = tabs.splice(from, 1);
      if (moved) tabs.splice(to, 0, moved);
      set({ tabs });
    },

    getText: (id) => texts.get(id)?.current,

    setText(id, text) {
      const entry = texts.get(id);
      const tab = find(id);
      if (!entry || !tab) return;
      entry.current = text;
      const dirty = text !== entry.saved;
      // Typing doesn't change the state; only becoming dirty or clean does.
      if (dirty !== tab.dirty) update(id, { dirty });
    },

    save,

    async saveAll() {
      const dirty = state.tabs.filter((tab) => tab.dirty);
      const results = await Promise.all(dirty.map((tab) => save(tab.id)));
      return results.every(Boolean);
    },

    requestClose(ids) {
      if (state.pendingClose) return Promise.resolve(false);
      const targets = state.tabs.filter((tab) => ids.includes(tab.id));
      const dirty = targets.filter((tab) => tab.dirty);
      const all = new Set(targets.map((tab) => tab.id));
      if (dirty.length === 0) {
        closeNow(all);
        return Promise.resolve(true);
      }
      return new Promise<boolean>((resolve) => {
        const finish = (closed: boolean) => {
          answer = null;
          set({ pendingClose: null });
          resolve(closed);
        };
        answer = async (choice) => {
          if (choice === "cancel") return finish(false);
          if (choice === "discard") {
            closeNow(all);
            return finish(true);
          }
          if (state.pendingClose?.saving) return;
          set({ pendingClose: { dirty, saving: true } });
          const saved = await Promise.all(dirty.map((tab) => save(tab.id)));
          const failed = dirty.filter((_, index) => !saved[index]).map((tab) => tab.id);
          for (const id of failed) all.delete(id);
          closeNow(all);
          const [firstFailed] = failed;
          if (firstFailed) set({ activeId: firstFailed });
          finish(failed.length === 0);
        };
        set({ pendingClose: { dirty, saving: false } });
      });
    },

    async answerClose(choice) {
      await answer?.(choice);
    },

    hasUnsaved: () => state.tabs.some((tab) => tab.dirty),

    async checkDisk(paths) {
      const targets = state.tabs.filter(
        (tab) => paths === null || paths.some((path) => within(tab.path, path)),
      );
      await Promise.all(targets.map((tab) => check(tab.id)));
    },

    async reload(id) {
      const tab = find(id);
      if (!tab || tab.saving) return false;
      const result = await port.readFile(tab.path);
      if (!find(id)) return false;
      if (!result.ok) {
        if (result.error.code === "notFound") update(id, { disk: "deleted" });
        options.onError?.(`Couldn't reload ${tab.name}`, result.error);
        return false;
      }
      adopt(id, result.value);
      return true;
    },

    keepMine(id) {
      if (find(id)?.disk === "changed") update(id, { disk: "same" });
    },
  };
}
