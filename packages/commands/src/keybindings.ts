// Keybindings: which keys run which command, written once, matched against
// key presses, and shown in menus and the palette in the platform's own way.
import type { CommandRegistry, Dispose } from "@repo/kernel";

/** macOS writes and matches keys its own way: Command for Mod, and ⌘-style labels. */
export type Platform = "mac" | "other";

/**
 * One binding, as written in the app's table. `key` is modifiers and one key
 * joined by `+`: `Mod+Shift+P`, `F1`, `Ctrl+PageDown`, `Mod+,`.
 *
 * - `Mod` is Ctrl, or Command on macOS. `Ctrl` is always the Control key.
 * - `Alt` (Option on macOS), `Shift`, `Meta` (Command, or the Windows key).
 * - The key is a letter, a digit, `F1`–`F12`, a named key (`PageDown`,
 *   `Enter`, `Escape`, `Tab`, `Delete`, `Home`, `End`, `ArrowUp`…), or one of
 *   `, . / ; ' [ ] \\ - = \``. Letters and symbols are matched by position on
 *   the keyboard (`KeyboardEvent.code`), so Shift and the layout don't change them.
 */
export interface KeybindingSpec<Id extends string> {
  readonly key: string;
  readonly command: Id;
}

interface Chord {
  readonly ctrl: boolean;
  readonly alt: boolean;
  readonly shift: boolean;
  readonly meta: boolean;
  /** `KeyboardEvent.code`. */
  readonly code: string;
}

/** A binding ready to match and show. */
export interface Keybinding<Id extends string> {
  readonly command: Id;
  /** As written in the table. */
  readonly key: string;
  /** For people: `Ctrl+Shift+P`, or `⇧⌘P` on macOS. */
  readonly label: string;
  /** For `aria-keyshortcuts`: `Control+Shift+P`, or `Meta+Shift+P` on macOS. */
  readonly aria: string;
  readonly chord: Chord;
}

/** Something wrong with the table: a key two commands claim, or one that can't be read. */
export type KeybindingProblem<Id extends string> =
  | {
      readonly kind: "conflict";
      readonly label: string;
      /** The command that keeps the key (the first in the table), then the others. */
      readonly commands: readonly Id[];
    }
  | {
      readonly kind: "invalid";
      readonly key: string;
      readonly command: Id;
      readonly reason: string;
    };

/** The fields of a key press that matching reads. */
export interface KeyPress {
  readonly code: string;
  readonly ctrlKey: boolean;
  readonly altKey: boolean;
  readonly shiftKey: boolean;
  readonly metaKey: boolean;
}

export interface Keybindings<Id extends string> {
  readonly platform: Platform;
  /** Every usable binding, in table order. */
  readonly all: readonly Keybinding<Id>[];
  /** The first binding of a command: what menus and the palette show. */
  forCommand(command: Id): Keybinding<Id> | undefined;
  /** The binding a key press matches, if any. */
  match(press: KeyPress): Keybinding<Id> | undefined;
  /** Conflicts and unreadable keys. Bindings that lose a conflict aren't matched. */
  readonly problems: readonly KeybindingProblem<Id>[];
}

const NAMED: Readonly<Record<string, string>> = {
  ",": "Comma",
  ".": "Period",
  "/": "Slash",
  ";": "Semicolon",
  "'": "Quote",
  "[": "BracketLeft",
  "]": "BracketRight",
  "\\": "Backslash",
  "-": "Minus",
  "=": "Equal",
  "`": "Backquote",
  Space: "Space",
};
const KEYS = new Set([
  "Enter",
  "Escape",
  "Tab",
  "Backspace",
  "Delete",
  "Insert",
  "Home",
  "End",
  "PageUp",
  "PageDown",
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
]);
/** How each code is shown, where it isn't simply the key's name. */
const SHOWN: Readonly<Record<string, string>> = Object.fromEntries([
  ...Object.entries(NAMED).map(([key, code]) => [code, key]),
]);
/** What `aria-keyshortcuts` calls each code: `KeyboardEvent.key` values. */
const ARIA: Readonly<Record<string, string>> = { ...SHOWN, Space: "Space" };

/** The platform this page runs on. */
export function detectPlatform(nav: Pick<Navigator, "userAgent"> = navigator): Platform {
  return /Mac|iPhone|iPad/.test(nav.userAgent) ? "mac" : "other";
}

function codeOf(key: string): string | null {
  if (/^[a-z]$/i.test(key)) return `Key${key.toUpperCase()}`;
  if (/^[0-9]$/.test(key)) return `Digit${key}`;
  if (/^F([1-9]|1[0-2])$/.test(key)) return key;
  if (KEYS.has(key)) return key;
  return NAMED[key] ?? null;
}

function parse(key: string, platform: Platform): Chord | string {
  // "Mod++" isn't needed: "=" and "-" are written as themselves.
  const parts = key.split("+");
  const last = parts.pop();
  if (!last) return "no key";
  const code = codeOf(last);
  if (!code) return `"${last}" isn't a key this table knows`;
  const chord = { ctrl: false, alt: false, shift: false, meta: false, code };
  for (const part of parts) {
    switch (part) {
      case "Mod":
        if (platform === "mac") chord.meta = true;
        else chord.ctrl = true;
        break;
      case "Ctrl":
        chord.ctrl = true;
        break;
      case "Alt":
        chord.alt = true;
        break;
      case "Shift":
        chord.shift = true;
        break;
      case "Meta":
        chord.meta = true;
        break;
      default:
        return `"${part}" isn't a modifier`;
    }
  }
  return chord;
}

const keyName = (code: string): string =>
  SHOWN[code] ?? code.replace(/^Key/, "").replace(/^Digit/, "");

function label(chord: Chord, platform: Platform): string {
  const key = keyName(chord.code);
  if (platform === "mac") {
    // macOS order and symbols: Control, Option, Shift, Command, then the key.
    return `${chord.ctrl ? "⌃" : ""}${chord.alt ? "⌥" : ""}${chord.shift ? "⇧" : ""}${chord.meta ? "⌘" : ""}${key}`;
  }
  return [
    chord.ctrl && "Ctrl",
    chord.alt && "Alt",
    chord.shift && "Shift",
    chord.meta && "Win",
    key,
  ]
    .filter(Boolean)
    .join("+");
}

function aria(chord: Chord): string {
  const key = ARIA[chord.code] ?? keyName(chord.code);
  return [
    chord.ctrl && "Control",
    chord.meta && "Meta",
    chord.alt && "Alt",
    chord.shift && "Shift",
    key,
  ]
    .filter(Boolean)
    .join("+");
}

const same = (a: Chord, b: Chord | KeyPress): boolean =>
  "ctrlKey" in b
    ? a.code === b.code &&
      a.ctrl === b.ctrlKey &&
      a.alt === b.altKey &&
      a.shift === b.shiftKey &&
      a.meta === b.metaKey
    : a.code === b.code &&
      a.ctrl === b.ctrl &&
      a.alt === b.alt &&
      a.shift === b.shift &&
      a.meta === b.meta;

/**
 * Reads a table of bindings for a platform. Problems don't throw: a key two
 * commands claim stays with the first, and an unreadable one is left out.
 * Both are listed in `problems`.
 */
export function createKeybindings<Id extends string>(
  specs: readonly KeybindingSpec<Id>[],
  platform: Platform,
): Keybindings<Id> {
  const all: Keybinding<Id>[] = [];
  const problems: KeybindingProblem<Id>[] = [];
  const conflicts = new Map<Keybinding<Id>, Id[]>();
  for (const spec of specs) {
    const chord = parse(spec.key, platform);
    if (typeof chord === "string") {
      problems.push({ kind: "invalid", key: spec.key, command: spec.command, reason: chord });
      continue;
    }
    const taken = all.find((binding) => same(binding.chord, chord));
    if (taken) {
      const losers = conflicts.get(taken) ?? [];
      losers.push(spec.command);
      conflicts.set(taken, losers);
      continue;
    }
    all.push({
      command: spec.command,
      key: spec.key,
      label: label(chord, platform),
      aria: aria(chord),
      chord,
    });
  }
  for (const [kept, losers] of conflicts) {
    problems.push({ kind: "conflict", label: kept.label, commands: [kept.command, ...losers] });
  }
  return {
    platform,
    all,
    problems,
    forCommand: (command) => all.find((binding) => binding.command === command),
    match: (press) => all.find((binding) => same(binding.chord, press)),
  };
}

/** A key press inside a modal dialog belongs to the dialog. */
function inDialog(target: EventTarget | null): boolean {
  return (
    target instanceof Element && target.closest('[role="dialog"], [role="alertdialog"]') !== null
  );
}

/**
 * Listens for key presses on `target` (the window) and runs the bound
 * command, when it's enabled. A press that something else already handled
 * (`defaultPrevented`, as the editor's own keys are), or one inside a dialog,
 * is left alone; so is one whose command is disabled, so the key keeps
 * whatever it does by default.
 */
export function attachKeybindings<Id extends string>(
  target: Pick<Window, "addEventListener" | "removeEventListener">,
  keybindings: Keybindings<Id>,
  registry: CommandRegistry<Id>,
): Dispose {
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.defaultPrevented || event.isComposing || inDialog(event.target)) return;
    const binding = keybindings.match(event);
    if (!binding || !registry.isEnabled(binding.command)) return;
    event.preventDefault();
    // Failures are reported by the registry's `onError`.
    void registry.execute(binding.command);
  };
  target.addEventListener("keydown", onKeyDown);
  return () => target.removeEventListener("keydown", onKeyDown);
}
