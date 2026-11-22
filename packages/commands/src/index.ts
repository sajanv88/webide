// Public entry of @repo/commands: keybindings and the command palette, on the registry in @repo/kernel.
export { CommandPalette, type CommandPaletteProps, paletteGroups } from "./command-palette";
export {
  attachKeybindings,
  createKeybindings,
  detectPlatform,
  type Keybinding,
  type KeybindingProblem,
  type KeybindingSpec,
  type Keybindings,
  type KeyPress,
  type Platform,
} from "./keybindings";
