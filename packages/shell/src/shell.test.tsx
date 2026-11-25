import type { AppCommandId } from "@repo/contracts";
import { createCommandRegistry } from "@repo/kernel";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { MenuSpec } from "./app-menubar";
import { Shell } from "./shell";

const menus: MenuSpec[] = [
  {
    label: "File",
    items: [
      { kind: "command", command: "workspace.openFolder" },
      { kind: "command", command: "help.about", label: "About this app" },
      { kind: "separator" },
      { kind: "submenu", label: "Open Recent", items: [] },
      { kind: "separator" },
      { kind: "command", command: "file.save" },
    ],
  },
  { label: "Edit", items: [{ kind: "command", command: "tabs.close" }] },
];

function renderShell() {
  const registry = createCommandRegistry<AppCommandId>();
  const view = render(
    <Shell
      title="Test IDE"
      registry={registry}
      menus={menus}
      views={[
        {
          id: "explorer",
          title: "Explorer",
          icon: <span>E</span>,
          content: <p>explorer slot</p>,
          command: "view.showExplorer",
        },
        { id: "search", title: "Search", icon: <span>S</span>, content: <p>search slot</p> },
      ]}
      main={<p>main slot</p>}
      statusBar={<span>status slot</span>}
    />,
  );
  return { registry, ...view };
}

describe("Shell", () => {
  it("names every landmark and fills the slots", () => {
    renderShell();
    expect(screen.getByRole("banner", { name: "Title bar" })).toBeTruthy();
    expect(screen.getByRole("main", { name: "Workspace" })).toBeTruthy();
    expect(screen.getByRole("contentinfo", { name: "Status bar" })).toBeTruthy();
    expect(screen.getByRole("menubar", { name: "Application" })).toBeTruthy();
    expect(screen.getByRole("separator", { name: "Resize side bar" })).toBeTruthy();
    expect(screen.getByRole("heading", { level: 1, name: "Test IDE" })).toBeTruthy();
    for (const slot of ["explorer slot", "main slot", "status slot"]) {
      expect(screen.getByText(slot)).toBeTruthy();
    }
  });

  it("shows one view at a time, chosen in the activity bar, and keeps the others mounted", () => {
    renderShell();
    const bar = screen.getByRole("tablist", { name: "Views" });
    expect(bar.getAttribute("aria-orientation")).toBe("vertical");
    const explorer = within(bar).getByRole("tab", { name: "Explorer" });
    const search = within(bar).getByRole("tab", { name: "Search" });
    expect(explorer.getAttribute("aria-selected")).toBe("true");
    expect(screen.getByRole("tabpanel", { name: "Explorer" })).toBeTruthy();

    fireEvent.mouseDown(search);
    expect(search.getAttribute("aria-selected")).toBe("true");
    expect(screen.getByRole("tabpanel", { name: "Search" })).toBeTruthy();
    // The explorer's panel is still there, so its state is kept; it isn't drawn.
    expect(screen.getByText("explorer slot")).toBeTruthy();
  });

  it("registers a show command for views that name one, and view.showView for any", async () => {
    const { registry } = renderShell();
    expect(registry.get("view.showExplorer")?.title).toBe("Show Explorer");
    expect(registry.get("view.showView")?.palette).toBe(false);

    await act(() => registry.execute("view.showView", "search"));
    expect(screen.getByRole("tab", { name: "Search" }).getAttribute("aria-selected")).toBe("true");
    await act(() => registry.execute("view.showExplorer"));
    expect(screen.getByRole("tab", { name: "Explorer" }).getAttribute("aria-selected")).toBe(
      "true",
    );
    // An unknown view changes nothing.
    await act(() => registry.execute("view.showView", "nope"));
    expect(screen.getByRole("tab", { name: "Explorer" }).getAttribute("aria-selected")).toBe(
      "true",
    );
  });

  it("without views, the work area fills the window and there's nothing to toggle", () => {
    const registry = createCommandRegistry<AppCommandId>();
    render(<Shell title="Test IDE" registry={registry} menus={menus} main={<p>main slot</p>} />);
    expect(screen.getByRole("main", { name: "Workspace" })).toBeTruthy();
    expect(screen.queryByRole("separator", { name: "Resize side bar" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Show side bar" })).toBeNull();
    expect(screen.queryByRole("tablist", { name: "Views" })).toBeNull();
    expect(registry.get("view.toggleSidebar")).toBeUndefined();
  });

  it("registers the side bar commands while mounted", () => {
    const { registry, unmount } = renderShell();
    expect(registry.get("view.toggleSidebar")?.title).toBe("Toggle Side Bar");
    unmount();
    expect(registry.get("view.toggleSidebar")).toBeUndefined();
    expect(registry.get("view.showView")).toBeUndefined();
    expect(registry.get("view.showExplorer")).toBeUndefined();
  });

  it("builds menus from the registry: titles, enabled state and running", async () => {
    const { registry } = renderShell();
    const run = vi.fn();
    act(() => {
      registry.register({ id: "workspace.openFolder", title: "Open Folder…", run });
      registry.register({ id: "help.about", title: "About", isEnabled: () => false, run });
    });

    fireEvent.keyDown(screen.getByRole("menuitem", { name: "File" }), { key: "Enter" });

    const open = await screen.findByRole("menuitem", { name: "Open Folder…" });
    const about = screen.getByRole("menuitem", { name: "About this app" });
    const recent = screen.getByRole("menuitem", { name: "Open Recent" });
    expect(open.getAttribute("aria-disabled")).toBeNull();
    // Disabled by the registry, and an empty submenu: both disabled.
    expect(about.getAttribute("aria-disabled")).toBe("true");
    expect(recent.getAttribute("aria-disabled")).toBe("true");
    // Not registered: left out, with the separator before it.
    expect(screen.queryByRole("menuitem", { name: "file.save" })).toBeNull();
    expect(
      within(screen.getByRole("menu", { name: "File" })).getAllByRole("separator"),
    ).toHaveLength(1);

    fireEvent.click(open);
    expect(run).toHaveBeenCalledTimes(1);
  });

  it("keeps a menu with nothing registered in it, disabled", () => {
    renderShell();
    const edit = screen.getByRole("menuitem", { name: "Edit" });
    expect(edit.getAttribute("aria-disabled") ?? edit.getAttribute("data-disabled")).not.toBeNull();
  });

  it("shows each command's keys in the menu, and gives them to screen readers", async () => {
    const registry = createCommandRegistry<AppCommandId>();
    registry.register({ id: "workspace.openFolder", title: "Open Folder…", run: () => {} });
    render(
      <Shell
        title="Test IDE"
        registry={registry}
        menus={menus}
        main={<p>main</p>}
        shortcutFor={(command) =>
          command === "workspace.openFolder" ? { label: "Ctrl+O", aria: "Control+O" } : undefined
        }
      />,
    );
    fireEvent.keyDown(screen.getByRole("menuitem", { name: "File" }), { key: "Enter" });
    const item = await screen.findByRole("menuitem", { name: "Open Folder…" });
    expect(item.getAttribute("aria-keyshortcuts")).toBe("Control+O");
    expect(item.textContent).toBe("Open Folder…Ctrl+O");
  });

  it("puts the panel under the work area, named, with its commands", () => {
    const registry = createCommandRegistry<AppCommandId>();
    const { unmount } = render(
      <Shell
        title="Test IDE"
        registry={registry}
        menus={menus}
        main={<p>main slot</p>}
        panel={{ title: "Terminal", content: <p>panel slot</p> }}
      />,
    );
    expect(screen.getByRole("region", { name: "Terminal" }).textContent).toBe("panel slot");
    expect(screen.getByRole("separator", { name: "Resize panel" })).toBeTruthy();
    expect(registry.get("view.togglePanel")?.title).toBe("Toggle Panel");
    expect(registry.get("view.showPanel")?.palette).toBe(false);
    unmount();
    expect(registry.get("view.togglePanel")).toBeUndefined();
  });

  it("puts the aside after the work area, named, with a button and its commands", () => {
    const registry = createCommandRegistry<AppCommandId>();
    const { unmount } = render(
      <Shell
        title="Test IDE"
        registry={registry}
        menus={menus}
        main={<p>main slot</p>}
        aside={{ title: "Assistant", icon: <span>A</span>, content: <p>aside slot</p> }}
      />,
    );
    expect(screen.getByRole("complementary", { name: "Assistant" }).textContent).toBe("aside slot");
    expect(screen.getByRole("separator", { name: "Resize Assistant" })).toBeTruthy();
    const button = screen.getByRole("button", { name: "Show Assistant" });
    expect(button.getAttribute("aria-controls")).toBe("app-shell-aside");
    expect(registry.get("view.toggleAside")?.title).toBe("Toggle Secondary Side Bar");
    expect(registry.get("view.showAside")?.palette).toBe(false);
    unmount();
    expect(registry.get("view.toggleAside")).toBeUndefined();
  });
});
