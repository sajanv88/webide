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
      sidebar={<p>sidebar slot</p>}
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
    expect(screen.getByRole("separator", { name: "Resize explorer" })).toBeTruthy();
    expect(screen.getByRole("heading", { level: 1, name: "Test IDE" })).toBeTruthy();
    for (const slot of ["sidebar slot", "main slot", "status slot"]) {
      expect(screen.getByText(slot)).toBeTruthy();
    }
  });

  it("without a sidebar, the work area fills the window and there's nothing to toggle", () => {
    const registry = createCommandRegistry<AppCommandId>();
    render(<Shell title="Test IDE" registry={registry} menus={menus} main={<p>main slot</p>} />);
    expect(screen.getByRole("main", { name: "Workspace" })).toBeTruthy();
    expect(screen.queryByRole("separator", { name: "Resize explorer" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Show explorer" })).toBeNull();
    expect(registry.get("view.toggleExplorer")).toBeUndefined();
  });

  it("registers the explorer toggle while mounted", () => {
    const { registry, unmount } = renderShell();
    expect(registry.get("view.toggleExplorer")?.title).toBe("Toggle Explorer");
    unmount();
    expect(registry.get("view.toggleExplorer")).toBeUndefined();
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
});
