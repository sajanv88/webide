// Public entry of @repo/terminal: the only package that imports @xterm/*.
export { allowTerminalStyles } from "./csp";
export { TerminalPanel, type TerminalPanelProps } from "./terminal-panel";
export { fit, type TerminalStatus, TerminalView, type TerminalViewProps } from "./terminal-view";
export { luminance, terminalTheme, tokenColor } from "./theme";
