// The app's keys: the one table menus, the palette and key presses all read.
// `Mod` is Ctrl, or Command on macOS. The format is described in @repo/commands.
import type { KeybindingSpec } from "@repo/commands";
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
  { key: "Mod+P", command: "search.goToFile" },
  { key: "Mod+,", command: "settings.open" },
];
