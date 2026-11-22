import "./shell.scss";
import { SidebarIcon } from "@nuvui/icons";
import {
  Button,
  Resizable,
  ResizableHandle,
  ResizablePanel,
  useResizableLayout,
  useResizablePanelRef,
} from "@nuvui/react";
import type { AppCommandId } from "@repo/contracts";
import type { CommandRegistry } from "@repo/kernel";
import { type ReactNode, useEffect, useState } from "react";
import { AppMenubar, type MenuShortcut, type MenuSpec } from "./app-menubar";

/** DOM id of the explorer pane, which the toggle button controls. */
const SIDEBAR_ID = "app-shell-sidebar";

export interface ShellProps {
  /** Shown in the top bar: the app's name from its config, and the open folder. */
  readonly title: string;
  readonly registry: CommandRegistry<AppCommandId>;
  /** The menubar, built from commands in `registry`. */
  readonly menus: readonly MenuSpec[];
  /** The keys bound to each command, shown beside it in the menus. */
  readonly shortcutFor?: (command: AppCommandId) => MenuShortcut | undefined;
  /** Slot: the resizable pane at the start (the explorer). Without it, the work area fills the window. */
  readonly sidebar?: ReactNode;
  /** Slot: the main work area. */
  readonly main: ReactNode;
  /** Slot: items in the status bar. */
  readonly statusBar?: ReactNode;
}

/**
 * The window frame, after nuvui's app-shell block: a top bar (menus, title,
 * tools), a resizable side pane, the work area and a status bar, each a named
 * landmark. It fills slots; it doesn't know what's in them.
 */
export function Shell({
  title,
  registry,
  menus,
  shortcutFor,
  sidebar,
  main,
  statusBar,
}: ShellProps) {
  const hasSidebar = sidebar !== undefined && sidebar !== null;
  const sidebarPanel = useResizablePanelRef();
  const [sidebarOpen, setSidebarOpen] = useState(true);
  // Pane sizes are a per-machine convenience, kept in the webview's localStorage.
  const { defaultLayout, onLayoutChanged } = useResizableLayout({ id: "shell" });

  useEffect(() => {
    if (!hasSidebar) return;
    return registry.register({
      id: "view.toggleExplorer",
      title: "Toggle Explorer",
      category: "View",
      run: () => {
        const panel = sidebarPanel.current;
        if (!panel) return;
        if (panel.isCollapsed()) panel.expand();
        else panel.collapse();
      },
    });
  }, [registry, sidebarPanel, hasSidebar]);

  const mainArea = (
    <main className="app-shell__main" aria-label="Workspace">
      {main}
    </main>
  );

  return (
    <div className="app-shell">
      {/* biome-ignore lint/a11y/useAriaPropsSupportedByRole: a top-level <header> is the banner landmark (HTML-AAM), which takes a name; Biome can't see that it's top-level */}
      <header className="app-shell__bar" aria-label="Title bar">
        <AppMenubar registry={registry} menus={menus} shortcutFor={shortcutFor} />
        <h1 className="app-shell__title">{title}</h1>
        <div className="app-shell__tools">
          {hasSidebar ? (
            <Button
              intent="ghost"
              size="sm"
              className="app-shell__icon"
              aria-label="Show explorer"
              aria-expanded={sidebarOpen}
              aria-controls={SIDEBAR_ID}
              onClick={() => void registry.execute("view.toggleExplorer")}
            >
              <SidebarIcon size={18} />
            </Button>
          ) : null}
        </div>
      </header>
      <div className="app-shell__body">
        {hasSidebar ? (
          <Resizable
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
              {sidebar}
            </ResizablePanel>
            <ResizableHandle aria-label="Resize explorer" />
            <ResizablePanel id="app-shell-main" minSize="30%" className="app-shell__pane">
              {mainArea}
            </ResizablePanel>
          </Resizable>
        ) : (
          mainArea
        )}
      </div>
      {/* biome-ignore lint/a11y/useAriaPropsSupportedByRole: a top-level <footer> is the contentinfo landmark (HTML-AAM), which takes a name; Biome can't see that it's top-level */}
      <footer className="app-shell__status" aria-label="Status bar">
        {statusBar}
      </footer>
    </div>
  );
}
