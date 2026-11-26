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
import { type ReactNode, type RefObject, useCallback, useEffect, useRef, useState } from "react";
import { AppMenubar, type MenuShortcut, type MenuSpec } from "./app-menubar";
import { dragHandlers, WindowControls, type WindowFrame } from "./window-controls";

/** DOM id of the side pane, which the toggle button controls. */
const SIDEBAR_ID = "app-shell-sidebar";
/** DOM id of the pane at the end, which its button controls. */
const ASIDE_ID = "app-shell-aside";

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
  /**
   * Slot: a pane at the end of the work area (the assistant), hidden until
   * shown, resizable, named `title`. It stays mounted while it's hidden.
   * The shell registers `view.toggleAside` and `view.showAside` for it.
   */
  readonly aside?: {
    readonly title: string;
    /** On the button in the top bar that shows and hides it. */
    readonly icon: ReactNode;
    readonly content: ReactNode;
  };
  /** Slot: items in the status bar. */
  readonly statusBar?: ReactNode;
  /**
   * The window, when the shell draws its frame (there's no system title
   * bar): the top bar moves it, has its buttons, and a border around it
   * shows whether it has focus.
   */
  readonly frame?: WindowFrame;
}

/** Whether the page's window has focus, which the frame's border shows. */
function useWindowFocus(): boolean {
  const [focused, setFocused] = useState(() => document.hasFocus());
  useEffect(() => {
    const focus = () => setFocused(true);
    const blur = () => setFocused(false);
    window.addEventListener("focus", focus);
    window.addEventListener("blur", blur);
    return () => {
      window.removeEventListener("focus", focus);
      window.removeEventListener("blur", blur);
    };
  }, []);
  return focused;
}

/**
 * A pane that can hide: it starts hidden unless its size was kept from
 * before, opens at its last size (or `size`, when that was barely there),
 * and gives focus back to where it was when it hides with focus inside.
 */
function useHidingPane(enabled: boolean, kept: unknown, size: `${number}%`) {
  const panel = useResizablePanelRef();
  const section = useRef<HTMLElement>(null);
  // Where focus was before it went into the pane, to give back when it's hidden.
  const before = useRef<HTMLElement | null>(null);

  useEffect(() => {
    const element = section.current;
    if (!enabled || !element) return;
    const entered = (event: FocusEvent) => {
      const from = event.relatedTarget;
      if (from instanceof HTMLElement && !element.contains(from)) before.current = from;
    };
    element.addEventListener("focusin", entered);
    return () => element.removeEventListener("focusin", entered);
  }, [enabled]);

  const start = useRef(kept);
  useEffect(() => {
    if (enabled && !start.current) panel.current?.collapse();
  }, [enabled, panel]);

  const open = useCallback(() => {
    const now = panel.current;
    if (!now?.isCollapsed()) return;
    now.expand();
    if (now.getSize().asPercentage < 15) now.resize(size);
  }, [panel, size]);

  const toggle = useCallback(() => {
    const now = panel.current;
    if (!now) return;
    if (now.isCollapsed()) {
      open();
      return;
    }
    const hadFocus = section.current?.contains(document.activeElement) ?? false;
    now.collapse();
    // Focus mustn't stay in a hidden pane: back where it was before.
    if (hadFocus) {
      const back = before.current;
      if (back?.isConnected) back.focus();
      else (document.activeElement as HTMLElement | null)?.blur();
    }
  }, [panel, open]);

  return { panel, section: section as RefObject<HTMLElement | null>, open, toggle };
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
  aside,
  statusBar,
  frame,
}: ShellProps) {
  const focused = useWindowFocus();
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
  const panelLayout = useResizableLayout({ id: "shell-panel" });
  const hasPanel = panel !== undefined;
  const bottom = useHidingPane(hasPanel, panelLayout.defaultLayout, "30%");
  const asideLayout = useResizableLayout({ id: "shell-aside" });
  const hasAside = aside !== undefined;
  const end = useHidingPane(hasAside, asideLayout.defaultLayout, "30%");
  const [asideOpen, setAsideOpen] = useState(false);

  useEffect(() => {
    if (!hasPanel) return;
    const disposers = [
      registry.register({
        id: "view.togglePanel",
        title: "Toggle Panel",
        category: "View",
        run: bottom.toggle,
      }),
      registry.register({
        id: "view.showPanel",
        title: "Show Panel",
        category: "View",
        palette: false,
        run: bottom.open,
      }),
    ];
    return () => {
      for (const dispose of disposers) dispose();
    };
  }, [registry, hasPanel, bottom.toggle, bottom.open]);

  useEffect(() => {
    if (!hasAside) return;
    const disposers = [
      registry.register({
        id: "view.toggleAside",
        title: "Toggle Secondary Side Bar",
        category: "View",
        run: end.toggle,
      }),
      registry.register({
        id: "view.showAside",
        title: "Show Secondary Side Bar",
        category: "View",
        palette: false,
        run: end.open,
      }),
    ];
    return () => {
      for (const dispose of disposers) dispose();
    };
  }, [registry, hasAside, end.toggle, end.open]);

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
        panelRef={bottom.panel}
        collapsible
        defaultSize="30%"
        minSize="6rem"
        maxSize="80%"
        className="app-shell__pane"
      >
        <section ref={bottom.section} className="app-shell__panel" aria-label={panel.title}>
          {panel.content}
        </section>
      </ResizablePanel>
    </Resizable>
  ) : (
    workArea
  );
  const centre = aside ? (
    <Resizable
      className="app-shell__row"
      {...(asideLayout.defaultLayout ? { defaultLayout: asideLayout.defaultLayout } : {})}
      onLayoutChanged={asideLayout.onLayoutChanged}
    >
      <ResizablePanel id="app-shell-centre" minSize="30%" className="app-shell__pane">
        {mainArea}
      </ResizablePanel>
      <ResizableHandle aria-label={`Resize ${aside.title}`} />
      <ResizablePanel
        id={ASIDE_ID}
        panelRef={end.panel}
        onResize={(size) => setAsideOpen(size.asPercentage > 0)}
        collapsible
        defaultSize="30%"
        minSize="18rem"
        maxSize="60%"
        className="app-shell__pane"
      >
        <aside ref={end.section} className="app-shell__aside" aria-label={aside.title}>
          {aside.content}
        </aside>
      </ResizablePanel>
    </Resizable>
  ) : (
    mainArea
  );

  return (
    <div className={shellClass(frame, focused)}>
      {/* biome-ignore lint/a11y/useAriaPropsSupportedByRole: a top-level <header> is the banner landmark (HTML-AAM), which takes a name; Biome can't see that it's top-level */}
      <header className="app-shell__bar" aria-label="Title bar" {...dragHandlers(frame)}>
        <div className="app-shell__start">
          <AppMenubar registry={registry} menus={menus} shortcutFor={shortcutFor} />
        </div>
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
          {aside ? (
            <Button
              intent="ghost"
              size="sm"
              className="app-shell__icon"
              aria-label={`Show ${aside.title}`}
              title={aside.title}
              aria-expanded={asideOpen}
              aria-controls={ASIDE_ID}
              onClick={() => void registry.execute("view.toggleAside")}
            >
              {aside.icon}
            </Button>
          ) : null}
          {frame ? <WindowControls frame={frame} /> : null}
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
              {centre}
            </ResizablePanel>
          </Resizable>
        </Tabs>
      ) : (
        <div className="app-shell__body">{centre}</div>
      )}
      {/* biome-ignore lint/a11y/useAriaPropsSupportedByRole: a top-level <footer> is the contentinfo landmark (HTML-AAM), which takes a name; Biome can't see that it's top-level */}
      <footer className="app-shell__status" aria-label="Status bar">
        {statusBar}
      </footer>
    </div>
  );
}

/** The shell's classes: whether it draws the window's frame, and how. */
function shellClass(frame: WindowFrame | undefined, focused: boolean): string {
  if (!frame) return "app-shell";
  const classes = ["app-shell", "app-shell--framed", `app-shell--${frame.os}`];
  // A maximized or full screen window has no border, as the system's has none.
  if (!frame.maximized && !frame.fullscreen) classes.push("app-shell--bordered");
  if (frame.fullscreen) classes.push("app-shell--fullscreen");
  if (!focused) classes.push("app-shell--inactive");
  return classes.join(" ");
}
