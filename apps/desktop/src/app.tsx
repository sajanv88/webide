// The composition root: the only place that wires modules together.
import { BugIcon, FolderIcon, GitBranchIcon, SearchIcon, SparklesIcon } from "@nuvui/icons";
import { Toaster, toast } from "@nuvui/react";
import { AssistantPanel, createAssistantStore, SetupDialog } from "@repo/assistant";
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
  AssistantPort,
  DebugPort,
  EditorProblem,
  GitPort,
  IpcError,
  LanguagePort,
  RecentWorkspaceDto,
  SearchPort,
  TerminalPort,
  WindowStateDto,
  WorkspaceDto,
} from "@repo/contracts";
import { createDebugStore, DebugView, debuggable } from "@repo/debug";
import {
  DiffSurface,
  EditorSurface,
  type EditorSurfaceHandle,
  type SurfaceDisplay,
  type SurfaceSourceAction,
} from "@repo/editor-surface";
import { Explorer, type ExplorerSource } from "@repo/explorer";
import { BranchPicker, BranchStatus, createGitStore, SourceControlView } from "@repo/git";
import {
  agentConfigure,
  agentNewToken,
  agentStatus,
  appInfo,
  assistantCheck,
  assistantClear,
  assistantDecide,
  assistantForget,
  assistantModels,
  assistantSave,
  assistantSend,
  assistantStatus,
  assistantStop,
  closeTerminal,
  closeWindow,
  currentWorkspace,
  debugBreakpoints,
  debugEvaluate,
  debugPause,
  debugResume,
  debugScopes,
  debugStack,
  debugStart,
  debugStop,
  debugVariables,
  dragWindow,
  getSettings,
  getStyleNonce,
  gitBranches,
  gitCommit,
  gitDiff,
  gitDiscard,
  gitInit,
  gitStage,
  gitState,
  gitSwitchBranch,
  gitUnstage,
  listDirectory,
  listFiles,
  lspChange,
  lspClose,
  lspCompletions,
  lspDefinition,
  lspFixes,
  lspHover,
  lspOpen,
  lspProblems,
  lspResolveCompletion,
  lspRestart,
  lspSourceAction,
  minimizeWindow,
  openRecentWorkspace,
  openTerminal,
  pickWorkspace,
  readFile,
  recentWorkspaces,
  requestCloseWindow,
  resizeTerminal,
  searchText,
  setCloseGuard,
  subscribeEvents,
  toggleMaximizeWindow,
  updateSettings,
  windowState,
  writeFile,
  writeTerminal,
} from "@repo/ipc";
import { createCommandRegistry, createEventBus } from "@repo/kernel";
import { createLanguageClient, LanguageStatusItem } from "@repo/lsp";
import { AgentStatusItem, AgentsDialog, createAgentStore } from "@repo/mcp";
import { QuickOpen, SearchView } from "@repo/search";
import { SettingsDialog } from "@repo/settings";
import { type MenuSpec, Shell, type SidebarView, type WindowFrame } from "@repo/shell";
import { createDocuments, type Documents, DocumentTabs } from "@repo/tabs";
import { TerminalPanel } from "@repo/terminal";
import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { AboutDialog } from "./about/about-dialog";
import { DEFAULT_KEYBINDINGS, terminalPassesKey } from "./keybindings";
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

const passKey = terminalPassesKey(keybindings);

const settingsPort = { get: getSettings, update: updateSettings };

const applyTheme = (theme: string) => {
  document.documentElement.dataset.theme = theme;
};
bus.on("settings.changed", (settings) => applyTheme(settings.theme));

// The MCP server for AI agents: one for the app, whatever folder is open.
// The AI assistant: one for the app; its conversation is about the folder open.
const assistantPort: AssistantPort = {
  status: assistantStatus,
  save: assistantSave,
  forget: assistantForget,
  models: assistantModels,
  check: assistantCheck,
  send: assistantSend,
  stop: assistantStop,
  decide: assistantDecide,
  clear: assistantClear,
};
const assistant = createAssistantStore(assistantPort, bus);

const agents = createAgentStore(
  { status: agentStatus, configure: agentConfigure, newToken: agentNewToken },
  bus,
);

const noSubscribe = () => () => {};
const noState = () => null;

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
  const [windowShown, setWindowShown] = useState<WindowStateDto>({
    maximized: false,
    fullscreen: false,
  });
  const [workspace, setWorkspace] = useState<WorkspaceDto | null>(null);
  const [recent, setRecent] = useState<readonly RecentWorkspaceDto[]>([]);
  const [aboutOpen, setAboutOpen] = useState(false);
  // How the editor draws its lines, from the settings; its own default until read.
  const [display, setDisplay] = useState<SurfaceDisplay | undefined>(undefined);
  useEffect(() => bus.on("settings.changed", (settings) => setDisplay(settings.editor)), []);

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
        case "debug":
          if (event.workspaceId === current?.id) bus.emit("debug.event", event.event);
          break;
        case "languageStatus":
          if (event.workspaceId === current?.id) {
            bus.emit("language.status", { server: event.server, status: event.status });
          }
          break;
        case "agentCall":
          bus.emit("agent.call", event.call);
          break;
        case "windowState":
          setWindowShown(event.state);
          break;
        case "assistant":
          bus.emit("assistant.event", event.event);
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

  // Startup: app info, the workspace that survives a reload, the recent list,
  // the theme, and whether agents are on.
  useEffect(() => {
    let live = true;
    void agents.refresh();
    void assistant.refresh();
    void windowState().then((shown) => {
      if (live && shown.ok) setWindowShown(shown.value);
    });
    void Promise.all([appInfo(), currentWorkspace(), recentWorkspaces(), getSettings()]).then(
      ([nextInfo, current, nextRecent, settings]) => {
        if (!live) return;
        if (nextInfo.ok) setInfo(nextInfo.value);
        else console.error("Could not read the app info", nextInfo.error);
        if (current.ok) setWorkspace(current.value);
        if (nextRecent.ok) setRecent(nextRecent.value);
        if (settings.ok) {
          applyTheme(settings.value.theme);
          setDisplay(settings.value.editor);
        }
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

  const searchPort = useMemo<SearchPort | null>(
    () =>
      workspace && {
        searchText: (query) => searchText({ workspaceId: workspace.id, ...query }),
        listFiles: () => listFiles({ workspaceId: workspace.id }),
      },
    [workspace],
  );

  // Git for the open folder: one store, shared by the view, the status bar and the branch picker.
  const gitPort = useMemo<GitPort | null>(() => {
    if (!workspace) return null;
    const workspaceId = workspace.id;
    return {
      state: () => gitState({ workspaceId }),
      diff: (path, side) => gitDiff({ workspaceId, path, side }),
      stage: (paths) => gitStage({ workspaceId, paths: [...paths] }),
      unstage: (paths) => gitUnstage({ workspaceId, paths: [...paths] }),
      discard: (paths) => gitDiscard({ workspaceId, paths: [...paths] }),
      commit: (message) => gitCommit({ workspaceId, message }),
      branches: () => gitBranches({ workspaceId }),
      switchBranch: (name, create) => gitSwitchBranch({ workspaceId, name, create }),
      init: () => gitInit({ workspaceId }),
    };
  }, [workspace]);
  const git = useMemo(() => gitPort && createGitStore(gitPort), [gitPort]);

  const terminalPort = useMemo<TerminalPort | null>(() => {
    if (!workspace) return null;
    const workspaceId = workspace.id;
    return {
      open: (cols, rows, onEvent) => openTerminal({ workspaceId, cols, rows }, onEvent),
      write: writeTerminal,
      resize: resizeTerminal,
      close: closeTerminal,
    };
  }, [workspace]);

  // The language server for TypeScript and JavaScript files, in the open folder.
  const languagePort = useMemo<LanguagePort | null>(() => {
    if (!workspace) return null;
    const workspaceId = workspace.id;
    return {
      open: (path, text) => lspOpen({ workspaceId, path, text }),
      change: (path, text) => lspChange({ workspaceId, path, text }),
      close: (path) => lspClose({ workspaceId, path }),
      problems: (path) => lspProblems({ workspaceId, path }),
      completions: (path, line, character) =>
        lspCompletions({ workspaceId, path, line, character }),
      hover: (path, line, character) => lspHover({ workspaceId, path, line, character }),
      definition: (path, line, character) => lspDefinition({ workspaceId, path, line, character }),
      resolveCompletion: (path, resolve) => lspResolveCompletion({ workspaceId, path, resolve }),
      fixes: (path, start, end) => lspFixes({ workspaceId, path, start, end }),
      sourceAction: (path, action) => lspSourceAction({ workspaceId, path, action }),
      restart: () => lspRestart({ workspaceId }),
    };
  }, [workspace]);
  const language = useMemo(
    () => languagePort && createLanguageClient(languagePort, bus),
    [languagePort],
  );
  useEffect(() => () => language?.dispose(), [language]);
  // The problems of each open file, for its editor.
  const [problems, setProblems] = useState<ReadonlyMap<string, readonly EditorProblem[]>>(
    new Map(),
  );
  // biome-ignore lint/correctness/useExhaustiveDependencies: a new client (another folder) starts with no problems
  useEffect(() => {
    setProblems(new Map());
    return bus.on("language.problems", ({ path, problems: found }) =>
      setProblems((now) => new Map(now).set(path, found)),
    );
  }, [language]);

  // The debugger: one program at a time, in the open folder.
  const debugPort = useMemo<DebugPort | null>(() => {
    if (!workspace) return null;
    const workspaceId = workspace.id;
    return {
      start: (path, breakpoints) =>
        debugStart({
          workspaceId,
          path,
          breakpoints: breakpoints.map((file) => ({ path: file.path, lines: [...file.lines] })),
        }),
      stop: () => debugStop({ workspaceId }),
      setBreakpoints: (path, lines) => debugBreakpoints({ workspaceId, path, lines: [...lines] }),
      resume: (threadId, step) => debugResume({ workspaceId, threadId, ...(step ? { step } : {}) }),
      pause: (threadId) => debugPause({ workspaceId, threadId }),
      stack: (threadId) => debugStack({ workspaceId, threadId }),
      scopes: (frameId) => debugScopes({ workspaceId, reference: frameId }),
      variables: (reference) => debugVariables({ workspaceId, reference }),
      evaluate: (expression, frameId) =>
        debugEvaluate({ workspaceId, expression, ...(frameId === undefined ? {} : { frameId }) }),
    };
  }, [workspace]);
  const debug = useMemo(() => debugPort && createDebugStore(debugPort, bus), [debugPort]);
  useEffect(() => () => debug?.dispose(), [debug]);
  // Breakpoints and the paused line, for the editor.
  useSyncExternalStore(debug?.subscribe ?? noSubscribe, debug?.getState ?? noState);
  // The file the editor shows, which Run and Debug runs.
  const currentFile = useCallback(() => {
    const state = documentsRef.current?.getState();
    const tab = state?.tabs.find((candidate) => candidate.id === state.activeId);
    return tab?.content.kind === "text" ? tab.path : undefined;
  }, []);

  // The editor, for the commands that change the text it shows: quick fixes
  // and the source actions (Organize Imports…), from its language server.
  const editorHandle = useRef<EditorSurfaceHandle | null>(null);
  useEffect(() => {
    const handled = () => {
      const path = currentFile();
      return path !== undefined && language?.handles(path) === true;
    };
    const sourceActions: readonly {
      id: AppCommandId;
      action: SurfaceSourceAction;
      title: string;
      nothing: string;
    }[] = [
      {
        id: "editor.organizeImports",
        action: "organizeImports",
        title: "Organize Imports",
        nothing: "The imports are already organized",
      },
      {
        id: "editor.sortImports",
        action: "sortImports",
        title: "Sort Imports",
        nothing: "The imports are already sorted",
      },
      {
        id: "editor.removeUnusedImports",
        action: "removeUnusedImports",
        title: "Remove Unused Imports",
        nothing: "Every import is used",
      },
      { id: "editor.fixAll", action: "fixAll", title: "Fix All", nothing: "Nothing to fix" },
    ];
    const disposers = [
      registry.register({
        id: "editor.quickFix",
        title: "Quick Fix…",
        category: "Code",
        isEnabled: handled,
        run: () => editorHandle.current?.showFixes(),
      }),
      ...sourceActions.map(({ id, action, title, nothing }) =>
        registry.register({
          id,
          title,
          category: "Code",
          isEnabled: handled,
          run: async () => {
            const result = await editorHandle.current?.runSourceAction(action);
            if (result === "unchanged") toast(nothing);
            else if (result === "unavailable") toast(`${title} isn't offered for this file`);
            else if (result === "stale") {
              toast("The file changed meanwhile", { description: `Run ${title} again.` });
            }
          },
        }),
      ),
    ];
    return () => {
      for (const dispose of disposers) dispose();
    };
  }, [language, currentFile]);

  const views = useMemo<SidebarView[]>(
    () =>
      source && searchPort && workspace && git && gitPort
        ? [
            {
              id: "explorer",
              title: "Explorer",
              icon: <FolderIcon size={20} />,
              command: "view.showExplorer",
              content: (
                <Explorer source={source} registry={registry} bus={bus} styleNonce={styleNonce} />
              ),
            },
            {
              id: "search",
              title: "Search",
              icon: <SearchIcon size={20} />,
              content: (
                <SearchView
                  key={workspace.id}
                  port={searchPort}
                  registry={registry}
                  bus={bus}
                  viewId="search"
                  styleNonce={styleNonce}
                />
              ),
            },
            {
              id: "git",
              title: "Source Control",
              icon: <GitBranchIcon size={20} />,
              command: "view.showSourceControl",
              content: (
                <SourceControlView
                  key={workspace.id}
                  store={git}
                  readDiff={gitPort.diff}
                  registry={registry}
                  bus={bus}
                  viewId="git"
                  styleNonce={styleNonce}
                  renderDiff={(diff) => (
                    <DiffSurface
                      path={diff.path}
                      original={diff.original}
                      modified={diff.modified}
                      aria-label={diff.label}
                      styleNonce={styleNonce}
                    />
                  )}
                />
              ),
            },
          ]
        : [],
    [source, searchPort, workspace, git, gitPort],
  );
  const allViews = useMemo<SidebarView[]>(
    () =>
      debug && workspace
        ? [
            ...views,
            {
              id: "debug",
              title: "Run and Debug",
              icon: <BugIcon size={20} />,
              command: "view.showDebug",
              content: (
                <DebugView
                  key={workspace.id}
                  store={debug}
                  registry={registry}
                  currentFile={currentFile}
                  viewId="debug"
                  styleNonce={styleNonce}
                />
              ),
            },
          ]
        : views,
    [views, debug, workspace, currentFile],
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
          { kind: "command", command: "agents.open" },
          { kind: "command", command: "assistant.setup" },
        ],
      },
      {
        label: "View",
        items: [
          { kind: "command", command: "commands.showPalette" },
          { kind: "separator" },
          { kind: "command", command: "view.showExplorer" },
          { kind: "command", command: "search.findInFiles" },
          { kind: "command", command: "view.showSourceControl" },
          { kind: "command", command: "view.toggleSidebar" },
          { kind: "command", command: "view.togglePanel" },
          { kind: "command", command: "view.toggleAside" },
          { kind: "separator" },
          { kind: "command", command: "assistant.focus" },
          { kind: "command", command: "assistant.newChat" },
          { kind: "separator" },
          { kind: "command", command: "terminal.focus" },
          { kind: "command", command: "terminal.new" },
          { kind: "separator" },
          { kind: "command", command: "explorer.collapseAll" },
        ],
      },
      {
        label: "Go",
        items: [
          { kind: "command", command: "search.goToFile" },
          { kind: "separator" },
          { kind: "command", command: "tabs.next" },
          { kind: "command", command: "tabs.previous" },
        ],
      },
      {
        label: "Code",
        items: [
          { kind: "command", command: "editor.quickFix" },
          { kind: "separator" },
          { kind: "command", command: "editor.organizeImports" },
          { kind: "command", command: "editor.sortImports" },
          { kind: "command", command: "editor.removeUnusedImports" },
          { kind: "separator" },
          { kind: "command", command: "editor.fixAll" },
        ],
      },
      {
        label: "Run",
        items: [
          { kind: "command", command: "debug.start" },
          { kind: "command", command: "debug.stop" },
          { kind: "separator" },
          { kind: "command", command: "debug.continue" },
          { kind: "command", command: "debug.pause" },
          { kind: "command", command: "debug.stepOver" },
          { kind: "command", command: "debug.stepInto" },
          { kind: "command", command: "debug.stepOut" },
          { kind: "separator" },
          { kind: "command", command: "view.showDebug" },
        ],
      },
      { label: "Help", items: [{ kind: "command", command: "help.about" }] },
    ],
    [recent],
  );

  // The app draws its own title bar: the shell moves the window, and has its buttons.
  const frame = useMemo<WindowFrame | undefined>(
    () =>
      info
        ? {
            os: info.os,
            ...windowShown,
            minimize: () => void minimizeWindow(),
            toggleMaximize: () => void toggleMaximizeWindow(),
            close: () => void requestCloseWindow(),
            startDrag: () => void dragWindow(),
          }
        : undefined,
    [info, windowShown],
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
        {...(frame ? { frame } : {})}
        registry={registry}
        menus={menus}
        shortcutFor={keybindings.forCommand}
        views={allViews}
        {...(workspace && terminalPort
          ? {
              panel: {
                title: "Terminal",
                content: (
                  <TerminalPanel
                    key={workspace.id}
                    port={terminalPort}
                    registry={registry}
                    bus={bus}
                    passKey={passKey}
                    styleNonce={styleNonce}
                  />
                ),
              },
            }
          : {})}
        aside={{
          title: "Assistant",
          icon: <SparklesIcon size={18} />,
          content: (
            <AssistantPanel
              store={assistant}
              registry={registry}
              activePath={() => {
                const state = documents?.getState();
                return state?.tabs.find((tab) => tab.id === state.activeId)?.path;
              }}
              renderDiff={(path, original, proposed) => (
                <DiffSurface
                  path={path}
                  original={original}
                  modified={proposed}
                  aria-label={`The assistant's change to ${path}`}
                  styleNonce={styleNonce}
                />
              )}
            />
          ),
        }}
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
                  ref={(handle) => {
                    document.ref(handle);
                    editorHandle.current = handle;
                  }}
                  aria-label={`Text of ${document.path}`}
                  styleNonce={styleNonce}
                  display={display}
                  diagnostics={problems.get(document.path) ?? []}
                  language={
                    language?.handles(document.path) ? language.features(document.path) : undefined
                  }
                  {...(debug && debuggable(document.path)
                    ? {
                        breakpoints: debug.getState().breakpoints.get(document.path) ?? [],
                        onToggleBreakpoint: (line: number) =>
                          debug.toggleBreakpoint(document.path, line),
                        executionLine: debug.executionLine(document.path),
                      }
                    : {})}
                />
              )}
            />
          ) : (
            <Welcome recent={recent} registry={registry} />
          )
        }
        statusBar={
          <>
            {git ? <BranchStatus store={git} registry={registry} /> : null}
            {language ? (
              <LanguageStatusItem client={language} registry={registry} bus={bus} />
            ) : null}
            <AgentStatusItem store={agents} registry={registry} />
            <span>{workspace ? workspace.path : "No folder open"}</span>
            {info ? <span>Version {info.version}</span> : null}
          </>
        }
      />
      <AboutDialog info={info} open={aboutOpen} onOpenChange={setAboutOpen} />
      <CommandPalette registry={registry} keybindings={keybindings} />
      {workspace && searchPort ? (
        // Keys name the component too: siblings sharing the workspace's id
        // would be mixed up when it changes, and register their commands twice.
        <QuickOpen
          key={`quick-open-${workspace.id}`}
          port={searchPort}
          registry={registry}
          bus={bus}
        />
      ) : null}
      {workspace && git ? (
        <BranchPicker key={`branch-picker-${workspace.id}`} store={git} registry={registry} />
      ) : null}
      <SettingsDialog port={settingsPort} registry={registry} bus={bus} />
      <AgentsDialog store={agents} registry={registry} />
      <SetupDialog
        store={assistant}
        port={assistantPort}
        registry={registry}
        onSaved={() => void registry.execute("assistant.focus")}
      />
      <Toaster />
    </>
  );
}
