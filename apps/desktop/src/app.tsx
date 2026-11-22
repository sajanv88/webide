// The composition root: the only place that wires modules together.
import { Toaster, toast } from "@nuvui/react";
import {
  attachKeybindings,
  CommandPalette,
  createKeybindings,
  detectPlatform,
} from "@repo/commands";
import type {
  AppCommandId,
  AppEvents,
  AppInfoDto,
  IpcError,
  RecentWorkspaceDto,
  WorkspaceDto,
} from "@repo/contracts";
import { EditorSurface } from "@repo/editor-surface";
import { Explorer, type ExplorerSource } from "@repo/explorer";
import {
  appInfo,
  closeWindow,
  currentWorkspace,
  getSettings,
  getStyleNonce,
  listDirectory,
  openRecentWorkspace,
  pickWorkspace,
  readFile,
  recentWorkspaces,
  setCloseGuard,
  subscribeEvents,
  updateSettings,
  writeFile,
} from "@repo/ipc";
import { createCommandRegistry, createEventBus } from "@repo/kernel";
import { SettingsDialog } from "@repo/settings";
import { type MenuSpec, Shell } from "@repo/shell";
import { createDocuments, type Documents, DocumentTabs } from "@repo/tabs";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AboutDialog } from "./about/about-dialog";
import { DEFAULT_KEYBINDINGS } from "./keybindings";
import { NoFileOpen } from "./no-file-open/no-file-open";
import { Welcome } from "./welcome/welcome";

const styleNonce = getStyleNonce();

const registry = createCommandRegistry<AppCommandId>({
  onError: (id, error) => {
    console.error(`Command ${id} failed`, error);
    toast("Something went wrong", { intent: "danger", description: String(error) });
  },
});
const bus = createEventBus<AppEvents>();

const keybindings = createKeybindings(DEFAULT_KEYBINDINGS, detectPlatform());
for (const problem of keybindings.problems) console.warn("Keybinding problem", problem);

const settingsPort = { get: getSettings, update: updateSettings };

const applyTheme = (theme: string) => {
  document.documentElement.dataset.theme = theme;
};
bus.on("settings.changed", (settings) => applyTheme(settings.theme));

function showError(title: string, error: IpcError) {
  toast(title, { intent: "danger", description: error.message });
}

/** `{ path: string }`, as the "Open Recent" entries pass it. */
function pathArg(args: unknown): string | undefined {
  if (typeof args !== "object" || args === null || !("path" in args)) return undefined;
  return typeof args.path === "string" ? args.path : undefined;
}

export function App() {
  const [info, setInfo] = useState<AppInfoDto | null>(null);
  const [workspace, setWorkspace] = useState<WorkspaceDto | null>(null);
  const [recent, setRecent] = useState<readonly RecentWorkspaceDto[]>([]);
  const [aboutOpen, setAboutOpen] = useState(false);

  // The open files of the open folder. A new folder starts with none.
  const documents = useMemo<Documents | null>(
    () =>
      workspace &&
      createDocuments(
        {
          readFile: (path) => readFile({ workspaceId: workspace.id, path }),
          writeFile: (path, text) => writeFile({ workspaceId: workspace.id, path, text }),
        },
        { onError: showError },
      ),
    [workspace],
  );
  const documentsRef = useRef(documents);
  documentsRef.current = documents;
  const workspaceRef = useRef(workspace);
  workspaceRef.current = workspace;

  // Files with unsaved changes are saved or dropped, as the person chooses,
  // before they'd be lost: another folder opening, or the window closing.
  // Resolves to whether nothing unsaved is left.
  const settleUnsaved = useCallback(async (): Promise<boolean> => {
    const open = documentsRef.current;
    if (!open?.hasUnsaved()) return true;
    const dirty = open
      .getState()
      .tabs.filter((tab) => tab.dirty)
      .map((tab) => tab.id);
    return open.requestClose(dirty);
  }, []);

  // What the backend tells this page: changes on disk, and the window closing.
  useEffect(() => {
    const warned = new Set<string>();
    let stop: (() => void) | null = null;
    let live = true;
    void subscribeEvents((event) => {
      const current = workspaceRef.current;
      switch (event.kind) {
        case "filesChanged":
          if (event.workspaceId === current?.id) {
            bus.emit("workspace.filesChanged", { changes: event.changes, rescan: event.rescan });
          }
          break;
        case "watchFailed":
          if (event.workspaceId === current?.id && !warned.has(event.workspaceId)) {
            warned.add(event.workspaceId);
            toast("Changes made outside the app may not show", {
              intent: "warning",
              description: `Watching ${current.name} for changes failed: ${event.message}. Use Refresh Explorer to see them.`,
            });
          }
          break;
        case "closeRequested":
          void settleUnsaved().then(async (settled) => {
            if (!settled) return;
            const closed = await closeWindow();
            if (!closed.ok) showError("Couldn't close the window", closed.error);
          });
          break;
      }
    }).then((result) => {
      if (!result.ok) {
        console.error("Could not hear from the backend", result.error);
        return;
      }
      if (live) stop = result.value;
      else result.value();
    });
    return () => {
      live = false;
      stop?.();
    };
  }, [settleUnsaved]);

  // While a file has unsaved changes, closing the window asks first.
  useEffect(() => {
    let unsaved = documents?.hasUnsaved() ?? false;
    void setCloseGuard(unsaved);
    if (!documents) return;
    const unsubscribe = documents.subscribe(() => {
      const now = documents.hasUnsaved();
      if (now === unsaved) return;
      unsaved = now;
      void setCloseGuard(now);
    });
    return unsubscribe;
  }, [documents]);

  // Startup: app info, the workspace that survives a reload, the recent list, the theme.
  useEffect(() => {
    let live = true;
    void Promise.all([appInfo(), currentWorkspace(), recentWorkspaces(), getSettings()]).then(
      ([nextInfo, current, nextRecent, settings]) => {
        if (!live) return;
        if (nextInfo.ok) setInfo(nextInfo.value);
        else console.error("Could not read the app info", nextInfo.error);
        if (current.ok) setWorkspace(current.value);
        if (nextRecent.ok) setRecent(nextRecent.value);
        if (settings.ok) applyTheme(settings.value.theme);
      },
    );
    return () => {
      live = false;
    };
  }, []);

  useEffect(() => {
    const refreshRecent = async () => {
      const result = await recentWorkspaces();
      if (result.ok) setRecent(result.value);
    };
    // Unsaved changes are settled before another folder is opened: opening one
    // replaces the backend's open folder at once, so it can't wait until a
    // folder has been picked.
    const adopt = async (next: WorkspaceDto) => {
      setWorkspace(next);
      bus.emit("workspace.opened", next);
      await refreshRecent();
    };

    const disposers = [
      registry.register({
        id: "workspace.openFolder",
        title: "Open Folder…",
        category: "File",
        run: async () => {
          if (!(await settleUnsaved())) return;
          const result = await pickWorkspace();
          if (!result.ok) return showError("Couldn't open the folder", result.error);
          if (result.value) await adopt(result.value);
        },
      }),
      registry.register({
        id: "workspace.openRecent",
        title: "Open Recent Folder",
        category: "File",
        // It needs the folder: the File menu and the welcome screen give it.
        palette: false,
        run: async (args) => {
          const path = pathArg(args);
          if (!path) return;
          if (!(await settleUnsaved())) return;
          const result = await openRecentWorkspace({ path });
          if (!result.ok) {
            showError("Couldn't open the folder", result.error);
            return refreshRecent();
          }
          await adopt(result.value);
        },
      }),
      registry.register({
        id: "help.about",
        title: "About",
        category: "Help",
        run: () => setAboutOpen(true),
      }),
    ];
    return () => {
      for (const dispose of disposers) dispose();
    };
  }, [settleUnsaved]);

  // Keys run commands from anywhere in the window (dialogs keep their own).
  useEffect(() => attachKeybindings(window, keybindings, registry), []);

  const source = useMemo<ExplorerSource | null>(
    () =>
      workspace && {
        workspace,
        port: { listDirectory: (path) => listDirectory({ workspaceId: workspace.id, path }) },
      },
    [workspace],
  );

  const menus = useMemo<MenuSpec[]>(
    () => [
      {
        label: "File",
        items: [
          { kind: "command", command: "workspace.openFolder" },
          {
            kind: "submenu",
            label: "Open Recent",
            items: recent.map((entry) => ({
              kind: "command",
              command: "workspace.openRecent",
              label: `${entry.name} (${entry.path})`,
              args: { path: entry.path },
            })),
          },
          { kind: "separator" },
          { kind: "command", command: "file.save" },
          { kind: "command", command: "file.saveAll" },
          { kind: "separator" },
          { kind: "command", command: "tabs.close" },
          { kind: "command", command: "tabs.closeOthers" },
          { kind: "command", command: "tabs.closeAll" },
          { kind: "separator" },
          { kind: "command", command: "explorer.refresh" },
          { kind: "separator" },
          { kind: "command", command: "settings.open" },
        ],
      },
      {
        label: "View",
        items: [
          { kind: "command", command: "commands.showPalette" },
          { kind: "separator" },
          { kind: "command", command: "view.toggleExplorer" },
          { kind: "command", command: "explorer.collapseAll" },
          { kind: "separator" },
          { kind: "command", command: "tabs.next" },
          { kind: "command", command: "tabs.previous" },
        ],
      },
      { label: "Help", items: [{ kind: "command", command: "help.about" }] },
    ],
    [recent],
  );

  const appName = info?.name ?? "";
  const title = workspace ? `${workspace.name} — ${appName}` : appName;
  useEffect(() => {
    document.title = title;
  }, [title]);

  return (
    <>
      <Shell
        title={title}
        registry={registry}
        menus={menus}
        shortcutFor={keybindings.forCommand}
        {...(source
          ? {
              sidebar: (
                <Explorer source={source} registry={registry} bus={bus} styleNonce={styleNonce} />
              ),
            }
          : {})}
        main={
          workspace && documents ? (
            <DocumentTabs
              key={workspace.id}
              documents={documents}
              registry={registry}
              bus={bus}
              empty={<NoFileOpen />}
              renderEditor={(document) => (
                <EditorSurface
                  {...document}
                  aria-label={`Text of ${document.path}`}
                  styleNonce={styleNonce}
                />
              )}
            />
          ) : (
            <Welcome recent={recent} registry={registry} />
          )
        }
        statusBar={
          <>
            <span>{workspace ? workspace.path : "No folder open"}</span>
            {info ? <span>Version {info.version}</span> : null}
          </>
        }
      />
      <AboutDialog info={info} open={aboutOpen} onOpenChange={setAboutOpen} />
      <CommandPalette registry={registry} keybindings={keybindings} />
      <SettingsDialog port={settingsPort} registry={registry} bus={bus} />
      <Toaster />
    </>
  );
}
