import "./document-tabs.scss";
import "./document-tab.scss";
import { CloseIcon } from "@nuvui/icons";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
  VisuallyHidden,
} from "@nuvui/react";
import type { AppCommandId, AppEvents } from "@repo/contracts";
import { FileTypeIcon } from "@repo/file-icons";
import type { CommandRegistry, EventBus } from "@repo/kernel";
import {
  type DragEvent,
  type ReactNode,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import { CloseDialog } from "./close-dialog";
import { DocumentNotice } from "./document-notice";
import type { DocumentId, Documents, DocumentView } from "./documents";

/** The part of the editor's handle the tabs use. */
export interface EditorSlotHandle {
  /** Replaces the text of the document the editor shows now. */
  setValue(value: string): void;
  focus(): void;
  /** Puts the cursor at `line` and `column` (1-based, UTF-16 units), scrolls to it, and focuses. */
  revealPosition(line: number, column: number): void;
}

/** Where to put the cursor once a document requested by `file.openRequested` is shown. */
interface Reveal {
  readonly id: DocumentId;
  readonly line?: number | undefined;
  readonly column?: number | undefined;
}

/** What the app's editor is given for the document it shows. */
export interface EditorSlotProps {
  readonly documentId: DocumentId;
  /** Workspace-relative, `/`-separated. The language comes from it. */
  readonly path: string;
  /** The text to start from, the first time this document is shown. */
  readonly initialValue: string;
  readonly readOnly: boolean;
  /** Called with the whole text on each change. */
  readonly onChange: (value: string) => void;
  /** Ctrl+S or Cmd+S in the editor. */
  readonly onSave: () => void;
  /** Give it the editor's handle: a file reloaded from disk is put in through it. */
  readonly ref: (handle: EditorSlotHandle | null) => void;
}

export interface DocumentTabsProps {
  /** The open documents of one workspace, from `createDocuments`. */
  readonly documents: Documents;
  readonly registry: CommandRegistry<AppCommandId>;
  readonly bus: EventBus<AppEvents>;
  /** Draws the editor. It stays mounted while the documents change. */
  readonly renderEditor: (props: EditorSlotProps) => ReactNode;
  /** Shown while no document is open. */
  readonly empty: ReactNode;
}

const DRAG_TYPE = "application/x-webide-tab";

/** Each tab's label: its name, and its folder where two open files share a name. */
function labels(tabs: readonly DocumentView[]): Map<DocumentId, string> {
  const count = new Map<string, number>();
  for (const tab of tabs) count.set(tab.name, (count.get(tab.name) ?? 0) + 1);
  return new Map(
    tabs.map((tab) => {
      if ((count.get(tab.name) ?? 0) < 2) return [tab.id, tab.name];
      const folder = tab.path.slice(0, tab.path.length - tab.name.length - 1);
      return [tab.id, `${tab.name} — ${folder || "/"}`];
    }),
  );
}

/** The open files as tabs over one editor, and the question asked before closing unsaved ones. */
export function DocumentTabs({ documents, registry, bus, renderEditor, empty }: DocumentTabsProps) {
  const state = useSyncExternalStore(documents.subscribe, documents.getState);
  const listRef = useRef<HTMLDivElement>(null);
  const emptyRef = useRef<HTMLDivElement>(null);
  const lastText = useRef<DocumentId | null>(null);
  const editor = useRef<EditorSlotHandle | null>(null);
  // The revision of each document the editor holds (it keeps every one it has shown).
  const shownRevision = useRef(new Map<DocumentId, number>());
  // (A ref may be called with null and the handle again on any render, so
  // null doesn't mean a new editor. A new one reads each document's current
  // text anyway, so a revision it's told about again changes nothing.)
  // Where to put the cursor once the editor shows a document asked for by
  // `file.openRequested`, and the document the editor shows now.
  const pendingReveal = useRef<Reveal | null>(null);
  const shownId = useRef<DocumentId | undefined>(undefined);
  // Puts the cursor there if the editor is drawn and shows that document. The
  // editor's code loads the first time one is shown, so its handle can come
  // after the document: then this runs again when it does.
  const applyReveal = useCallback(() => {
    const want = pendingReveal.current;
    const handle = editor.current;
    if (!want || !handle || want.id !== shownId.current) return;
    pendingReveal.current = null;
    if (want.line === undefined) handle.focus();
    else handle.revealPosition(want.line, want.column ?? 1);
  }, []);
  const editorRef = useCallback(
    (handle: EditorSlotHandle | null) => {
      editor.current = handle;
      if (handle) applyReveal();
    },
    [applyReveal],
  );

  // After a close, focus goes to the tab now active, or to the "nothing open"
  // slot when none is left, rather than falling back to the page.
  const focusActiveTab = useCallback(() => {
    requestAnimationFrame(() => {
      const tab = listRef.current?.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]');
      (tab ?? emptyRef.current)?.focus();
    });
  }, []);

  const close = useCallback(
    async (ids: readonly DocumentId[], refocus: boolean) => {
      const closed = await documents.requestClose(ids);
      if (closed && refocus) focusActiveTab();
    },
    [documents, focusActiveTab],
  );

  // Commands, and keeping their enabled state current.
  useEffect(() => {
    const active = () => {
      const { tabs, activeId } = documents.getState();
      return tabs.find((tab) => tab.id === activeId);
    };
    const all = () => documents.getState().tabs.map((tab) => tab.id);
    // The tab after (or before) the active one, round from the end to the start.
    const step = (by: 1 | -1) => {
      const { tabs, activeId } = documents.getState();
      const at = tabs.findIndex((tab) => tab.id === activeId);
      const next = tabs[(at + by + tabs.length) % tabs.length];
      if (next) documents.activate(next.id);
    };
    const disposers = [
      registry.register({
        id: "file.save",
        title: "Save",
        category: "File",
        isEnabled: () => {
          const tab = active();
          return tab?.content.kind === "text" && tab.content.readOnly === null;
        },
        run: async () => {
          const tab = active();
          if (tab) await documents.save(tab.id);
        },
      }),
      registry.register({
        id: "file.saveAll",
        title: "Save All",
        category: "File",
        isEnabled: () => documents.hasUnsaved(),
        run: async () => {
          await documents.saveAll();
        },
      }),
      registry.register({
        id: "tabs.close",
        title: "Close Editor",
        category: "Editor",
        isEnabled: () => active() !== undefined,
        run: async () => {
          const tab = active();
          if (tab) await close([tab.id], false);
        },
      }),
      registry.register({
        id: "tabs.closeOthers",
        title: "Close Other Editors",
        category: "Editor",
        isEnabled: () => documents.getState().tabs.length > 1,
        run: async () => {
          const keep = active()?.id;
          await close(
            all().filter((id) => id !== keep),
            false,
          );
        },
      }),
      registry.register({
        id: "tabs.closeAll",
        title: "Close All Editors",
        category: "Editor",
        isEnabled: () => documents.getState().tabs.length > 0,
        run: async () => {
          await close(all(), false);
        },
      }),
      registry.register({
        id: "tabs.next",
        title: "Next Editor",
        category: "Editor",
        isEnabled: () => documents.getState().tabs.length > 1,
        run: () => step(1),
      }),
      registry.register({
        id: "tabs.previous",
        title: "Previous Editor",
        category: "Editor",
        isEnabled: () => documents.getState().tabs.length > 1,
        run: () => step(-1),
      }),
    ];
    const unsubscribe = documents.subscribe(() => registry.notify());
    return () => {
      unsubscribe();
      for (const dispose of disposers) dispose();
    };
  }, [documents, registry, close]);

  // Tell the rest of the app which files are open, and their text (for the
  // language server): opened once their text is read, changed as it's typed
  // or reloaded, closed when their tab goes.
  const known = useRef(new Map<DocumentId, { path: string; text: string }>());
  const syncOpen = useCallback(() => {
    const seen = new Set<DocumentId>();
    for (const tab of documents.getState().tabs) {
      if (tab.content.kind !== "text") continue;
      const text = documents.getText(tab.id);
      if (text === undefined) continue;
      seen.add(tab.id);
      const before = known.current.get(tab.id);
      if (!before) {
        known.current.set(tab.id, { path: tab.path, text });
        bus.emit("document.opened", { path: tab.path, text });
      } else if (before.text !== text) {
        before.text = text;
        bus.emit("document.changed", { path: tab.path, text });
      }
    }
    for (const [id, doc] of known.current) {
      if (seen.has(id)) continue;
      known.current.delete(id);
      bus.emit("document.closed", { path: doc.path });
    }
  }, [documents, bus]);
  useEffect(() => {
    const open = known.current;
    syncOpen();
    const unsubscribe = documents.subscribe(syncOpen);
    return () => {
      unsubscribe();
      for (const doc of open.values()) bus.emit("document.closed", { path: doc.path });
      open.clear();
    };
  }, [documents, bus, syncOpen]);

  // A file chosen in the explorer opens here.
  useEffect(
    () =>
      bus.on("explorer.fileActivated", ({ path }) => {
        void documents.open(path);
      }),
    [bus, documents],
  );

  // A file asked for elsewhere (a search result, Go to File) opens here too,
  // and the editor takes focus, at the position given, once it shows it.
  const [revealAsked, setRevealAsked] = useState(0);
  useEffect(() => {
    let latest = 0;
    return bus.on("file.openRequested", ({ path, line, column }) => {
      const request = ++latest;
      void documents.open(path).then((id) => {
        if (!id || request !== latest) return;
        pendingReveal.current = { id, line, column };
        setRevealAsked((count) => count + 1);
      });
    });
  }, [bus, documents]);

  // Files changed on disk, by anyone: compare the open ones with their files.
  useEffect(
    () =>
      bus.on("workspace.filesChanged", ({ changes, rescan }) => {
        const paths = changes.flatMap((change) =>
          change.kind === "renamed" ? [change.from, change.path] : [change.path],
        );
        if (rescan || paths.length > 0) void documents.checkDisk(rescan ? null : paths);
      }),
    [bus, documents],
  );

  // Reloading the page would lose unsaved changes: ask first.
  useEffect(() => {
    const guard = (event: BeforeUnloadEvent) => {
      if (documents.hasUnsaved()) event.preventDefault();
    };
    window.addEventListener("beforeunload", guard);
    return () => window.removeEventListener("beforeunload", guard);
  }, [documents]);

  const active = state.tabs.find((tab) => tab.id === state.activeId);
  // The editor stays mounted while a notice or a loading file is shown, so
  // each document keeps its undo history, selection and scroll.
  if (active?.content.kind === "text") lastText.current = active.id;
  const editorTab = state.tabs.find(
    (tab) => tab.id === lastText.current && tab.content.kind === "text",
  );
  const editorId = editorTab?.id;
  const onChange = useCallback(
    (value: string) => {
      if (!editorId) return;
      documents.setText(editorId, value);
      // Typing doesn't change the store's state, so it isn't heard there.
      syncOpen();
    },
    [documents, editorId, syncOpen],
  );
  const onSave = useCallback(() => {
    if (editorId) void documents.save(editorId);
  }, [documents, editorId]);
  const names = useMemo(() => labels(state.tabs), [state.tabs]);

  // A document reloaded from disk: the editor gets the new text when it shows
  // it (now, or when its tab is next chosen). It runs after the editor has
  // switched to the document, since a child's effects run first.
  const editorRevision = editorTab?.revision;
  useEffect(() => {
    if (!editorId || editorRevision === undefined) return;
    const seen = shownRevision.current.get(editorId);
    shownRevision.current.set(editorId, editorRevision);
    if (seen === undefined || seen === editorRevision) return;
    const text = documents.getText(editorId);
    if (text !== undefined) editor.current?.setValue(text);
  }, [documents, editorId, editorRevision]);

  // After the editor has switched to the document (a child's effects run first).
  // biome-ignore lint/correctness/useExhaustiveDependencies: `revealAsked` is how a new request asks for this to run
  useEffect(() => {
    shownId.current = editorId;
    applyReveal();
  }, [editorId, revealAsked, applyReveal]);

  const dialog = (
    <CloseDialog
      pending={state.pendingClose}
      onAnswer={(choice) => void documents.answerClose(choice)}
      onClosed={focusActiveTab}
    />
  );

  if (!active) {
    return (
      <>
        <div ref={emptyRef} className="document-tabs__empty" tabIndex={-1}>
          {empty}
        </div>
        {dialog}
      </>
    );
  }

  return (
    <>
      <Tabs
        className="document-tabs"
        value={active.id}
        onValueChange={(value) => documents.activate(value as DocumentId)}
      >
        <TabsList ref={listRef} className="document-tabs__list" aria-label="Open files">
          {state.tabs.map((tab, index) => (
            <DocumentTab
              key={tab.id}
              tab={tab}
              label={names.get(tab.id) ?? tab.name}
              index={index}
              count={state.tabs.length}
              documents={documents}
              onClose={(ids, refocus) => void close(ids, refocus)}
            />
          ))}
        </TabsList>
        {/* One panel, for the active tab: the editor isn't remounted when the tab changes. */}
        <TabsContent
          value={active.id}
          className="document-tabs__panel"
          // A panel with the editor in it isn't a Tab stop of its own; one with
          // only a notice is, so the notice can be reached.
          tabIndex={active.content.kind === "text" ? -1 : 0}
        >
          <DocumentNotice
            tab={active}
            onReload={() => void documents.reload(active.id)}
            onKeepMine={() => documents.keepMine(active.id)}
          />
          {editorTab ? (
            <div className="document-tabs__editor" hidden={editorTab.id !== active.id}>
              {renderEditor({
                documentId: editorTab.id,
                path: editorTab.path,
                initialValue: documents.getText(editorTab.id) ?? "",
                readOnly: editorTab.content.kind !== "text" || editorTab.content.readOnly !== null,
                onChange,
                onSave,
                ref: editorRef,
              })}
            </div>
          ) : null}
        </TabsContent>
      </Tabs>
      {dialog}
    </>
  );
}

interface DocumentTabProps {
  readonly tab: DocumentView;
  readonly label: string;
  readonly index: number;
  readonly count: number;
  readonly documents: Documents;
  readonly onClose: (ids: readonly DocumentId[], refocus: boolean) => void;
}

function DocumentTab({ tab, label, index, count, documents, onClose }: DocumentTabProps) {
  const others = () =>
    documents
      .getState()
      .tabs.map((other) => other.id)
      .filter((id) => id !== tab.id);

  const onDrop = (event: DragEvent) => {
    const dragged = event.dataTransfer.getData(DRAG_TYPE) as DocumentId;
    if (!dragged) return;
    event.preventDefault();
    documents.reorder(dragged, index);
  };

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <div
          className={[
            "document-tab",
            tab.dirty ? "document-tab--dirty" : "",
            tab.disk === "deleted" ? "document-tab--deleted" : "",
            tab.disk === "changed" ? "document-tab--conflict" : "",
          ]
            .filter(Boolean)
            .join(" ")}
        >
          <TabsTrigger
            value={tab.id}
            className="document-tab__trigger"
            aria-keyshortcuts="Delete"
            draggable
            onDragStart={(event) => {
              event.dataTransfer.setData(DRAG_TYPE, tab.id);
              event.dataTransfer.effectAllowed = "move";
            }}
            onDragOver={(event) => {
              if (event.dataTransfer.types.includes(DRAG_TYPE)) event.preventDefault();
            }}
            onDrop={onDrop}
            onKeyDown={(event) => {
              if (event.key !== "Delete") return;
              event.preventDefault();
              onClose([tab.id], true);
            }}
          >
            <FileTypeIcon name={tab.name} size={16} />
            <span className="document-tab__name">{label}</span>
            {tab.dirty ? <VisuallyHidden>, unsaved changes</VisuallyHidden> : null}
            {tab.disk === "deleted" ? <VisuallyHidden>, deleted on disk</VisuallyHidden> : null}
            {tab.disk === "changed" ? <VisuallyHidden>, changed on disk</VisuallyHidden> : null}
          </TabsTrigger>
          {/* For the mouse. The keyboard closes a tab with Delete, the context
              menu or File → Close Editor; a button inside a tab list isn't allowed. */}
          <button
            type="button"
            className="document-tab__close"
            tabIndex={-1}
            aria-hidden="true"
            onClick={() => onClose([tab.id], false)}
          >
            <span className="document-tab__dot" />
            <CloseIcon className="document-tab__x" size={14} />
          </button>
        </div>
      </ContextMenuTrigger>
      <ContextMenuContent aria-label={`${tab.name} tab`}>
        <ContextMenuItem onSelect={() => onClose([tab.id], true)}>Close</ContextMenuItem>
        <ContextMenuItem disabled={count < 2} onSelect={() => onClose(others(), true)}>
          Close Others
        </ContextMenuItem>
        <ContextMenuItem onSelect={() => onClose([tab.id, ...others()], true)}>
          Close All
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem
          disabled={index === 0}
          onSelect={() => documents.reorder(tab.id, index - 1)}
        >
          Move Left
        </ContextMenuItem>
        <ContextMenuItem
          disabled={index === count - 1}
          onSelect={() => documents.reorder(tab.id, index + 1)}
        >
          Move Right
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}
