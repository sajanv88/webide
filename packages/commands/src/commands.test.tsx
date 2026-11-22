import type { AppCommandId } from "@repo/contracts";
import { createCommandRegistry } from "@repo/kernel";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { CommandPalette } from "./command-palette";
import { attachKeybindings, createKeybindings, detectPlatform, type KeyPress } from "./keybindings";

const press = (code: string, mods: Partial<Omit<KeyPress, "code">> = {}): KeyPress => ({
  code,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  metaKey: false,
  ...mods,
});

describe("keybindings", () => {
  const table = [
    { key: "Mod+Shift+P", command: "commands.showPalette" },
    { key: "F1", command: "commands.showPalette" },
    { key: "Mod+S", command: "file.save" },
    { key: "Mod+Alt+S", command: "file.saveAll" },
    { key: "Ctrl+PageDown", command: "tabs.next" },
    { key: "Mod+,", command: "settings.open" },
  ] as const;

  it("reads Mod as Ctrl, and shows keys the Windows and Linux way", () => {
    const keys = createKeybindings<AppCommandId>(table, "other");
    expect(keys.problems).toEqual([]);
    expect(keys.forCommand("commands.showPalette")).toMatchObject({
      label: "Ctrl+Shift+P",
      aria: "Control+Shift+P",
    });
    expect(keys.forCommand("settings.open")).toMatchObject({ label: "Ctrl+,", aria: "Control+," });
    expect(keys.match(press("KeyS", { ctrlKey: true }))?.command).toBe("file.save");
    expect(keys.match(press("KeyS", { ctrlKey: true, altKey: true }))?.command).toBe(
      "file.saveAll",
    );
    expect(keys.match(press("KeyS", { metaKey: true }))).toBeUndefined();
    expect(keys.match(press("F1"))?.command).toBe("commands.showPalette");
    // The letter's position, whatever Shift makes of it.
    expect(keys.match(press("KeyP", { ctrlKey: true, shiftKey: true }))?.command).toBe(
      "commands.showPalette",
    );
  });

  it("reads Mod as Command on macOS, and shows keys the macOS way", () => {
    const keys = createKeybindings<AppCommandId>(table, "mac");
    expect(keys.forCommand("commands.showPalette")).toMatchObject({
      label: "⇧⌘P",
      aria: "Meta+Shift+P",
    });
    expect(keys.forCommand("file.saveAll")?.label).toBe("⌥⌘S");
    // Ctrl stays the Control key.
    expect(keys.forCommand("tabs.next")?.label).toBe("⌃PageDown");
    expect(keys.match(press("KeyS", { metaKey: true }))?.command).toBe("file.save");
    expect(keys.match(press("KeyS", { ctrlKey: true }))).toBeUndefined();
  });

  it("reports a key two commands claim, and one it can't read; the first claim wins", () => {
    const keys = createKeybindings<AppCommandId>(
      [
        { key: "Mod+W", command: "tabs.close" },
        { key: "Ctrl+W", command: "tabs.closeAll" },
        { key: "Mod+Hyper+X", command: "help.about" },
        { key: "Mod+Banana", command: "help.about" },
      ],
      "other",
    );
    expect(keys.problems).toEqual([
      {
        kind: "invalid",
        key: "Mod+Hyper+X",
        command: "help.about",
        reason: `"Hyper" isn't a modifier`,
      },
      {
        kind: "invalid",
        key: "Mod+Banana",
        command: "help.about",
        reason: `"Banana" isn't a key this table knows`,
      },
      { kind: "conflict", label: "Ctrl+W", commands: ["tabs.close", "tabs.closeAll"] },
    ]);
    expect(keys.match(press("KeyW", { ctrlKey: true }))?.command).toBe("tabs.close");
    // On macOS they're different keys, so there's no conflict.
    const mac = createKeybindings<AppCommandId>(
      [
        { key: "Mod+W", command: "tabs.close" },
        { key: "Ctrl+W", command: "tabs.closeAll" },
      ],
      "mac",
    );
    expect(mac.problems).toEqual([]);
  });

  it("detects macOS from the user agent", () => {
    expect(detectPlatform({ userAgent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_0)" })).toBe(
      "mac",
    );
    expect(detectPlatform({ userAgent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Edg/140" })).toBe(
      "other",
    );
  });

  it("runs an enabled command on its key, and leaves handled, disabled and dialog keys alone", async () => {
    let canSave = true;
    const saved = vi.fn();
    const registry = createCommandRegistry<AppCommandId>();
    registry.register({ id: "file.save", title: "Save", isEnabled: () => canSave, run: saved });
    const keys = createKeybindings<AppCommandId>([{ key: "Mod+S", command: "file.save" }], "other");
    const dispose = attachKeybindings(window, keys, registry);

    const send = (target: EventTarget, init: KeyboardEventInit = {}) => {
      const event = new KeyboardEvent("keydown", {
        code: "KeyS",
        ctrlKey: true,
        bubbles: true,
        cancelable: true,
        ...init,
      });
      target.dispatchEvent(event);
      return event;
    };

    expect(send(document.body).defaultPrevented).toBe(true);
    await waitFor(() => expect(saved).toHaveBeenCalledTimes(1));

    // Handled already (the editor's own Ctrl+S): not run twice.
    const editor = document.createElement("div");
    document.body.append(editor);
    editor.addEventListener("keydown", (event) => event.preventDefault());
    send(editor);
    // Inside a dialog.
    const dialog = document.createElement("div");
    dialog.setAttribute("role", "dialog");
    document.body.append(dialog);
    send(dialog);
    // Disabled: the key keeps whatever it does by default.
    canSave = false;
    expect(send(document.body).defaultPrevented).toBe(false);
    await Promise.resolve();
    expect(saved).toHaveBeenCalledTimes(1);

    dispose();
    canSave = true;
    send(document.body);
    await Promise.resolve();
    expect(saved).toHaveBeenCalledTimes(1);
    editor.remove();
    dialog.remove();
  });

  it("takes a global key before the element with focus, which never sees it", async () => {
    const shown = vi.fn();
    const registry = createCommandRegistry<AppCommandId>();
    registry.register({ id: "view.showSourceControl", title: "Show Source Control", run: shown });
    const keys = createKeybindings<AppCommandId>(
      [{ key: "Ctrl+Shift+G", command: "view.showSourceControl", global: true }],
      "other",
    );
    const dispose = attachKeybindings(window, keys, registry);
    // An editor with its own use for the key (CodeMirror's "find previous").
    const editor = document.createElement("div");
    document.body.append(editor);
    const editorSaw = vi.fn((event: KeyboardEvent) => event.preventDefault());
    editor.addEventListener("keydown", editorSaw);

    const event = new KeyboardEvent("keydown", {
      code: "KeyG",
      ctrlKey: true,
      shiftKey: true,
      bubbles: true,
      cancelable: true,
    });
    editor.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
    expect(editorSaw).not.toHaveBeenCalled();
    await waitFor(() => expect(shown).toHaveBeenCalledTimes(1));
    expect(keys.forCommand("view.showSourceControl")?.label).toBe("Ctrl+Shift+G");

    dispose();
    editor.remove();
  });
});

describe("CommandPalette", () => {
  function setup() {
    const registry = createCommandRegistry<AppCommandId>();
    const saved = vi.fn();
    let canClose = false;
    registry.register({ id: "file.save", title: "Save", category: "File", run: saved });
    registry.register({
      id: "tabs.close",
      title: "Close Editor",
      category: "Editor",
      isEnabled: () => canClose,
      run: () => {},
    });
    registry.register({
      id: "workspace.openRecent",
      title: "Open Recent Folder",
      category: "File",
      palette: false,
      run: () => {},
    });
    registry.register({ id: "help.about", title: "About", run: () => {} });
    const keys = createKeybindings<AppCommandId>(
      [
        { key: "Mod+Shift+P", command: "commands.showPalette" },
        { key: "Mod+S", command: "file.save" },
      ],
      "other",
    );
    render(<CommandPalette registry={registry} keybindings={keys} />);
    return {
      registry,
      saved,
      enableClose: () => {
        canClose = true;
        registry.notify();
      },
    };
  }

  it("registers the command that opens it, and lists what can run now, by category", async () => {
    const { registry } = setup();
    expect(registry.get("commands.showPalette")?.title).toBe("Command Palette…");
    await act(async () => {
      await registry.execute("commands.showPalette");
    });
    const dialog = await screen.findByRole("dialog", { name: "Command palette" });
    const groups = within(dialog)
      .getAllByRole("group")
      .map((group) =>
        within(group)
          .getAllByRole("option")
          .map((option) => option.textContent),
      );
    // Categories in alphabetical order, "Other" last. Close Editor is disabled, Open Recent Folder needs arguments, and the
    // palette doesn't list itself.
    expect(groups).toEqual([["SaveCtrl+S"], ["About"]]);
    expect(
      within(dialog).getByRole("option", { name: "Save" }).getAttribute("aria-keyshortcuts"),
    ).toBe("Control+S");
    expect(document.activeElement).toBe(within(dialog).getByRole("combobox"));
  });

  it("finds a command by typing, runs it on Enter, and closes", async () => {
    const { registry, saved, enableClose } = setup();
    enableClose();
    await act(async () => {
      await registry.execute("commands.showPalette");
    });
    const dialog = await screen.findByRole("dialog", { name: "Command palette" });
    expect(within(dialog).getByRole("option", { name: "Close Editor" })).toBeTruthy();
    const input = within(dialog).getByRole("combobox");
    fireEvent.change(input, { target: { value: "file sav" } });
    await waitFor(() => expect(within(dialog).getAllByRole("option")).toHaveLength(1));
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(saved).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });
});
