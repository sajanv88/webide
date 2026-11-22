import "./shell.scss";
import { SidebarIcon } from "@nuvui/icons";
import {
  Button,
  Resizable,
  ResizableHandle,
  ResizablePanel,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
  useResizableLayout,
  useResizablePanelRef,
} from "@nuvui/react";
import type { AppCommandId } from "@repo/contracts";
import type { CommandRegistry } from "@repo/kernel";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { AppMenubar, type MenuShortcut, type MenuSpec } from "./app-menubar";

/** DOM id of the side pane, which the toggle button controls. */
const SIDEBAR_ID = "app-shell-sidebar";

/** One view of the side pane (the explorer, search…), chosen in the activity bar. */
export interface SidebarView {
  /** Unique among the views; `view.showView` takes it. */
  readonly id: string;
  /** The activity bar's name for it, and its panel's. */
  readonly title: string;
  readonly icon: ReactNode;
  readonly content: ReactNode;
  /** When given, the shell registers this command, which shows the view. */
  readonly command?: AppCommandId;
}

export interface ShellProps {
  /** Shown in the top bar: the app's name from its config, and the open folder. */
  readonly title: string;
  readonly registry: CommandRegistry<AppCommandId>;
  /** The menubar, built from commands in `registry`. */
  readonly menus: readonly MenuSpec[];
  /** The keys bound to each command, shown beside it in the menus. */
  readonly shortcutFor?: (command: AppCommandId) => MenuShortcut | undefined;
  /**
   * Slot: the views of the resizable pane at the start, the first shown first.
   * Without any, the work area fills the window.
   */
  readonly views?: readonly SidebarView[];
  /** Slot: the main work area. */
  readonly main: ReactNode;
  /**
   * Slot: a pane under the work area (the terminals), hidden until shown,
   * resizable, named `title`. It stays mounted while it's hidden.
   */
  readonly panel?: { readonly title: string; readonly content: ReactNode };
  /** Slot: items in the status bar. */
  readonly statusBar?: ReactNode;
}

/**
 * The window frame, after nuvui's app-shell block: a top bar (menus, title,
 * tools), an activity bar choosing the view in a resizable side pane, the
 * work area and a status bar, each a named landmark. It fills slots; it
 * doesn't know what's in them.
 */
export function Shell({
  title,
  registry,
  menus,
  shortcutFor,
  views = [],
  main,
  panel,
  statusBar,
}: ShellProps) {
  const first = views[0];
  const sidebarPanel = useResizablePanelRef();
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [chosen, setChosen] = useState<string | undefined>(first?.id);
  // A view that went away (or none chosen yet) falls back to the first.
  const active = views.some((view) => view.id === chosen) ? chosen : first?.id;
  // Pane sizes are a per-machine convenience, kept in the webview's localStorage.
  const { defaultLayout, onLayoutChanged } = useResizableLayout({ id: "shell" });
  // Whether the view under the pointer was already shown when it was pressed.
  const pressedActive = useRef(false);
  const bottomPanel = useResizablePanelRef();
  const panelSection = useRef<HTMLElement>(null);
  // Where focus was before it went into the panel, to give back when it's hidden.
  const beforePanel = useRef<HTMLElement | null>(null);
  const panelLayout = useResizableLayout({ id: "shell-panel" });
  const hasPanel = panel !== undefined;

  // Remember where focus came from when it enters the panel.
  useEffect(() => {
    const section = panelSection.current;
    if (!hasPanel || !section) return;
    const entered = (event: FocusEvent) => {
      const from = event.relatedTarget;
      if (from instanceof HTMLElement && !section.contains(from)) beforePanel.current = from;
    };
    section.addEventListener("focusin", entered);
    return () => section.removeEventListener("focusin", entered);
  }, [hasPanel]);

  // The panel starts hidden, unless its size was kept from before.
  const panelStart = useRef(panelLayout.defaultLayout);
  useEffect(() => {
    if (hasPanel && !panelStart.current) bottomPanel.current?.collapse();
  }, [hasPanel, bottomPanel]);

  useEffect(() => {
    if (!hasPanel) return;
    // Opened at its last size, or at 30% when that was barely there.
    const open = () => {
      const now = bottomPanel.current;
      if (!now?.isCollapsed()) return;
      now.expand();
      if (now.getSize().asPercentage < 15) now.resize("30%");
    };
    const disposers = [
      registry.register({
        id: "view.togglePanel",
        title: "Toggle Panel",
        category: "View",
        run: () => {
          const now = bottomPanel.current;
          if (!now) return;
          if (now.isCollapsed()) {
            open();
            return;
          }
          const hadFocus = panelSection.current?.contains(document.activeElement) ?? false;
          now.collapse();
          // Focus mustn't stay in a hidden panel: back where it was before.
          if (hadFocus) {
            const back = beforePanel.current;
            if (back?.isConnected) back.focus();
            else (document.activeElement as HTMLElement | null)?.blur();
          }
        },
      }),
      registry.register({
        id: "view.showPanel",
        title: "Show Panel",
        category: "View",
        palette: false,
        run: open,
      }),
    ];
    return () => {
      for (const dispose of disposers) dispose();
    };
  }, [registry, bottomPanel, hasPanel]);

  const viewIds = views.map((view) => `${view.id}:${view.command ?? ""}`).join(" ");
  // biome-ignore lint/correctness/useExhaustiveDependencies: `viewIds` stands for `views`, which is a new array on each render
  useEffect(() => {
    if (!first) return;
    const show = (id: string) => {
      setChosen(id);
      const panel = sidebarPanel.current;
      if (panel?.isCollapsed()) panel.expand();
    };
    const disposers = [
      registry.register({
        id: "view.toggleSidebar",
        title: "Toggle Side Bar",
        category: "View",
        run: () => {
          const panel = sidebarPanel.current;
          if (!panel) return;
          if (panel.isCollapsed()) panel.expand();
          else panel.collapse();
        },
      }),
      registry.register({
        id: "view.showView",
        title: "Show View",
        category: "View",
        palette: false,
        run: (id) => {
          if (typeof id === "string" && views.some((view) => view.id === id)) show(id);
        },
      }),
      ...views.flatMap((view) =>
        view.command
          ? [
              registry.register({
                id: view.command,
                title: `Show ${view.title}`,
                category: "View",
                run: () => show(view.id),
              }),
            ]
          : [],
      ),
    ];
    return () => {
      for (const dispose of disposers) dispose();
    };
  }, [registry, sidebarPanel, viewIds]);

  const workArea = (
    <main className="app-shell__main" aria-label="Workspace">
      {main}
    </main>
  );
  const mainArea = panel ? (
    <Resizable
      orientation="vertical"
      className="app-shell__column"
      {...(panelLayout.defaultLayout ? { defaultLayout: panelLayout.defaultLayout } : {})}
      onLayoutChanged={panelLayout.onLayoutChanged}
    >
      <ResizablePanel id="app-shell-work" minSize="20%" className="app-shell__pane">
        {workArea}
      </ResizablePanel>
      <ResizableHandle aria-label="Resize panel" />
      <ResizablePanel
        id="app-shell-panel"
        panelRef={bottomPanel}
        collapsible
        defaultSize="30%"
        minSize="6rem"
        maxSize="80%"
        className="app-shell__pane"
      >
        <section ref={panelSection} className="app-shell__panel" aria-label={panel.title}>
          {panel.content}
        </section>
      </ResizablePanel>
    </Resizable>
  ) : (
    workArea
  );

  return (
    <div className="app-shell">
      {/* biome-ignore lint/a11y/useAriaPropsSupportedByRole: a top-level <header> is the banner landmark (HTML-AAM), which takes a name; Biome can't see that it's top-level */}
      <header className="app-shell__bar" aria-label="Title bar">
        <AppMenubar registry={registry} menus={menus} shortcutFor={shortcutFor} />
        <h1 className="app-shell__title">{title}</h1>
        <div className="app-shell__tools">
          {first ? (
            <Button
              intent="ghost"
              size="sm"
              className="app-shell__icon"
              aria-label="Show side bar"
              aria-expanded={sidebarOpen}
              aria-controls={SIDEBAR_ID}
              onClick={() => void registry.execute("view.toggleSidebar")}
            >
              <SidebarIcon size={18} />
            </Button>
          ) : null}
        </div>
      </header>
      {first && active ? (
        <Tabs
          className="app-shell__body app-shell__body--views"
          orientation="vertical"
          value={active}
          onValueChange={setChosen}
        >
          <TabsList className="app-shell__activity" aria-label="Views">
            {views.map((view) => (
              <TabsTrigger
                key={view.id}
                value={view.id}
                className="app-shell__activity-item"
                aria-label={view.title}
                title={view.title}
                onPointerDown={() => {
                  pressedActive.current = view.id === active;
                }}
                onClick={(event) => {
                  // Clicking the view that's shown hides or shows the pane, as
                  // the toggle does. (Keys only move between views.)
                  if (event.detail === 0) return;
                  const panel = sidebarPanel.current;
                  if (panel?.isCollapsed()) panel.expand();
                  else if (pressedActive.current) panel?.collapse();
                }}
              >
                {view.icon}
              </TabsTrigger>
            ))}
          </TabsList>
          <Resizable
            className="app-shell__panes"
            {...(defaultLayout ? { defaultLayout } : {})}
            onLayoutChanged={onLayoutChanged}
          >
            <ResizablePanel
              id={SIDEBAR_ID}
              panelRef={sidebarPanel}
              collapsible
              defaultSize="22%"
              minSize="12rem"
              maxSize="60%"
              onResize={(size) => setSidebarOpen(size.asPercentage > 0)}
              className="app-shell__pane"
            >
              {views.map((view) => (
                // Every view stays mounted, so each keeps its state (a tree's
                // open folders, a search's results) while another is shown.
                <TabsContent
                  key={view.id}
                  value={view.id}
                  forceMount
                  className="app-shell__view"
                  tabIndex={-1}
                >
                  {view.content}
                </TabsContent>
              ))}
            </ResizablePanel>
            <ResizableHandle aria-label="Resize side bar" />
            <ResizablePanel id="app-shell-main" minSize="30%" className="app-shell__pane">
              {mainArea}
            </ResizablePanel>
          </Resizable>
        </Tabs>
      ) : (
        <div className="app-shell__body">{mainArea}</div>
      )}
      {/* biome-ignore lint/a11y/useAriaPropsSupportedByRole: a top-level <footer> is the contentinfo landmark (HTML-AAM), which takes a name; Biome can't see that it's top-level */}
      <footer className="app-shell__status" aria-label="Status bar">
        {statusBar}
      </footer>
    </div>
  );
}
