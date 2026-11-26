// The app's keys: the one table menus, the palette and key presses all read.
// `Mod` is Ctrl, or Command on macOS. The format is described in @repo/commands.
import type { KeybindingSpec, Keybindings } from "@repo/commands";
import type { AppCommandId } from "@repo/contracts";

export const DEFAULT_KEYBINDINGS: readonly KeybindingSpec<AppCommandId>[] = [
  { key: "Mod+Shift+P", command: "commands.showPalette" },
  { key: "F1", command: "commands.showPalette" },
  { key: "Mod+O", command: "workspace.openFolder" },
  { key: "Mod+S", command: "file.save" },
  { key: "Mod+Alt+S", command: "file.saveAll" },
  { key: "Mod+W", command: "tabs.close" },
  { key: "Ctrl+PageDown", command: "tabs.next" },
  { key: "Ctrl+PageUp", command: "tabs.previous" },
  { key: "Mod+B", command: "view.toggleSidebar" },
  { key: "Mod+Shift+E", command: "view.showExplorer" },
  { key: "Mod+Shift+F", command: "search.findInFiles" },
  // Global: in the editor, CodeMirror would take it as "find previous" (Shift+F3 does that too).
  { key: "Ctrl+Shift+G", command: "view.showSourceControl", global: true },
  { key: "Mod+P", command: "search.goToFile" },
  { key: "Ctrl+`", command: "terminal.focus" },
  { key: "Ctrl+Shift+`", command: "terminal.new" },
  { key: "Mod+Shift+D", command: "view.showDebug" },
  { key: "F5", command: "debug.start" },
  { key: "Shift+F5", command: "debug.stop" },
  { key: "F10", command: "debug.stepOver" },
  { key: "F11", command: "debug.stepInto" },
  { key: "Shift+F11", command: "debug.stepOut" },
  { key: "Mod+,", command: "settings.open" },
  { key: "Mod+.", command: "editor.quickFix" },
  { key: "Shift+Alt+O", command: "editor.organizeImports" },
  { key: "Mod+Alt+I", command: "assistant.focus" },
  { key: "Mod+Alt+B", command: "view.toggleAside" },
];

/**
 * The keys a terminal leaves to the app, as VS Code does: the palette, the
 * views and panels, Go to File, switching editors and settings. Everything
 * else (Ctrl+C, Ctrl+W, Ctrl+B…) goes to the shell, which has its own uses.
 */
const TERMINAL_PASSES: ReadonlySet<AppCommandId> = new Set<AppCommandId>([
  "commands.showPalette",
  "search.goToFile",
  "search.findInFiles",
  "view.showExplorer",
  "view.showSourceControl",
  "view.toggleSidebar",
  "view.toggleAside",
  "assistant.focus",
  "terminal.focus",
  "terminal.new",
  "tabs.next",
  "tabs.previous",
  "settings.open",
  "view.showDebug",
  "debug.start",
  "debug.stop",
  "debug.stepOver",
  "debug.stepInto",
  "debug.stepOut",
]);

/** Whether a key pressed in a terminal is the app's. */
export function terminalPassesKey(
  keybindings: Keybindings<AppCommandId>,
): (event: KeyboardEvent) => boolean {
  return (event) => {
    const binding = keybindings.match(event);
    return binding !== undefined && TERMINAL_PASSES.has(binding.command);
  };
}
