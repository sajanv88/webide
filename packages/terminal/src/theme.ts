import type { ITheme } from "@xterm/xterm";

/** A color as `#rrggbb`, or `null` when the browser can't say. */
export type Hex = `#${string}`;

/**
 * The page's value of a color token (`--color-surface`), as `#rrggbb`:
 * xterm reads hex, and the tokens are `oklch()`. Drawn on a one-pixel canvas
 * and read back, which works for any color the browser knows.
 */
export function tokenColor(token: string, within: Element = document.documentElement): Hex | null {
  const probe = document.createElement("span");
  probe.style.color = `var(${token})`;
  probe.style.display = "none";
  within.append(probe);
  const color = getComputedStyle(probe).color;
  probe.remove();
  const canvas = document.createElement("canvas");
  canvas.width = 1;
  canvas.height = 1;
  const context = canvas.getContext("2d", { willReadFrequently: true });
  if (!context || !color) return null;
  context.fillStyle = color;
  context.fillRect(0, 0, 1, 1);
  const [r = 0, g = 0, b = 0] = context.getImageData(0, 0, 1, 1).data;
  return `#${[r, g, b].map((part) => part.toString(16).padStart(2, "0")).join("")}`;
}

/** 0 for black, 1 for white. */
export function luminance(hex: Hex): number {
  const value = Number.parseInt(hex.slice(1), 16);
  const channels = [(value >> 16) & 255, (value >> 8) & 255, value & 255].map((c) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  const [r = 0, g = 0, b = 0] = channels;
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

// The 16 ANSI colors, for a dark and a light background: each readable on
// its own background (the bright ones too). Shells and programs pick these
// by number, so they're colors, not tokens.
const DARK: ITheme = {
  black: "#3b4252",
  red: "#f87171",
  green: "#4ade80",
  yellow: "#facc15",
  blue: "#60a5fa",
  magenta: "#e879f9",
  cyan: "#22d3ee",
  white: "#d1d5db",
  brightBlack: "#6b7280",
  brightRed: "#fca5a5",
  brightGreen: "#86efac",
  brightYellow: "#fde047",
  brightBlue: "#93c5fd",
  brightMagenta: "#f0abfc",
  brightCyan: "#67e8f9",
  brightWhite: "#f9fafb",
};

const LIGHT: ITheme = {
  black: "#1f2937",
  red: "#b91c1c",
  green: "#15803d",
  yellow: "#a16207",
  blue: "#1d4ed8",
  magenta: "#a21caf",
  cyan: "#0e7490",
  white: "#4b5563",
  brightBlack: "#6b7280",
  brightRed: "#dc2626",
  brightGreen: "#16a34a",
  brightYellow: "#ca8a04",
  brightBlue: "#2563eb",
  brightMagenta: "#c026d3",
  brightCyan: "#0891b2",
  brightWhite: "#111827",
};

/** The terminal's colors: the page's surface and text, and ANSI colors to suit them. */
export function terminalTheme(within?: Element): ITheme {
  const background = tokenColor("--color-surface", within) ?? "#111827";
  const foreground = tokenColor("--color-surface-foreground", within) ?? "#f9fafb";
  const primary = tokenColor("--color-primary", within) ?? "#3b82f6";
  const dark = luminance(background) < 0.4;
  return {
    ...(dark ? DARK : LIGHT),
    background,
    foreground,
    cursor: foreground,
    cursorAccent: background,
    // The selection over the text, translucent so the text stays readable.
    selectionBackground: `${primary}66`,
  };
}
