import "@xterm/xterm/css/xterm.css";
import "./terminal-view.scss";
import type { TerminalPort } from "@repo/contracts";
import { Terminal } from "@xterm/xterm";
import { useEffect, useRef } from "react";
import { allowTerminalStyles } from "./csp";
import { terminalTheme } from "./theme";

/** What a terminal tells its panel. */
export type TerminalStatus =
  | { readonly kind: "starting" }
  | { readonly kind: "running"; readonly id: number; readonly shell: string }
  | { readonly kind: "exited"; readonly code: number | null }
  | { readonly kind: "failed"; readonly message: string };

export interface TerminalViewProps {
  readonly port: TerminalPort;
  /** Its name for a screen reader: "Terminal 1". */
  readonly label: string;
  readonly active: boolean;
  /** A screen reader reads the output as it comes (xterm's own mode). */
  readonly screenReader: boolean;
  /** Keys the app takes even here (its palette, its views); the rest go to the shell. */
  readonly passKey?: ((event: KeyboardEvent) => boolean) | undefined;
  readonly styleNonce?: string | undefined;
  readonly onStatus: (status: TerminalStatus) => void;
  /** Gets a function that focuses the terminal. */
  readonly onReady?: (focus: () => void) => void;
  /** The page's theme changed: read the colors again. */
  readonly themeKey?: string;
  /**
   * Typed at the shell's prompt once it's up (its output has paused), and
   * not run: no Enter is pressed.
   */
  readonly initialInput?: string | undefined;
}

/** How long the shell's output pauses before it's taken to be at its prompt. */
export const PROMPT_QUIET_MS = 400;

const FONT_SIZE = 13;
const LINE_HEIGHT = 1.2;

/** Columns and rows that fit `box`, from the size of one character cell. */
export function fit(
  box: { readonly width: number; readonly height: number },
  cell: { readonly width: number; readonly height: number },
): { cols: number; rows: number } | null {
  if (cell.width <= 0 || cell.height <= 0) return null;
  // Room for xterm's scrollbar on the end.
  const cols = Math.floor((box.width - 14) / cell.width);
  const rows = Math.floor(box.height / cell.height);
  return cols >= 2 && rows >= 1 ? { cols: Math.min(cols, 1000), rows: Math.min(rows, 500) } : null;
}

/** One terminal: xterm.js, wired to a shell through the port. */
export function TerminalView({
  port,
  label,
  active,
  screenReader,
  passKey,
  styleNonce,
  onStatus,
  onReady,
  themeKey,
  initialInput,
}: TerminalViewProps) {
  const host = useRef<HTMLDivElement>(null);
  const term = useRef<Terminal | null>(null);
  // Props the long-lived terminal reads, kept current without restarting it.
  const latest = useRef({ passKey, onStatus, onReady, label });
  latest.current = { passKey, onStatus, onReady, label };

  // The terminal and its shell: started once, ended when the view goes.
  // biome-ignore lint/correctness/useExhaustiveDependencies: started once; `latest` carries what changes
  useEffect(() => {
    const element = host.current;
    if (!element) return;
    allowTerminalStyles(styleNonce);
    const xterm = new Terminal({
      fontFamily: getComputedStyle(element).getPropertyValue("--font-mono").trim() || "monospace",
      fontSize: FONT_SIZE,
      lineHeight: LINE_HEIGHT,
      cursorBlink: !window.matchMedia("(prefers-reduced-motion: reduce)").matches,
      scrollback: 5000,
      screenReaderMode: screenReader,
      theme: terminalTheme(element),
      allowProposedApi: false,
    });
    term.current = xterm;
    xterm.open(element);
    xterm.textarea?.setAttribute("aria-label", latest.current.label);

    let id: number | null = null;
    let exited = false;
    let stop = () => {};
    const write = (data: string) => {
      if (id !== null && !exited) void port.write(id, data);
    };

    // Ctrl+C copies when there's a selection (else it's the shell's: stop).
    // Ctrl+Shift+C copies, Ctrl+V and Ctrl+Shift+V paste (through the
    // browser's own paste, which xterm takes). The app's own keys pass.
    xterm.attachCustomKeyEventHandler((event) => {
      if (event.type !== "keydown") return true;
      const mod = event.ctrlKey && !event.altKey && !event.metaKey;
      if (mod && event.code === "KeyC" && (event.shiftKey || xterm.hasSelection())) {
        const text = xterm.getSelection();
        if (text) void navigator.clipboard?.writeText(text);
        if (!event.shiftKey) xterm.clearSelection();
        event.preventDefault();
        return false;
      }
      if (mod && event.code === "KeyV") return false;
      if (latest.current.passKey?.(event)) return false;
      return true;
    });

    // The size that fits, measured from one character cell.
    const cellSize = () => {
      const measure = element.querySelector<HTMLElement>(".xterm-char-measure-element");
      const rect = measure?.getBoundingClientRect();
      const chars = measure?.textContent?.length ?? 0;
      if (!rect || chars === 0) return { width: 0, height: 0 };
      // A drawn row is the true height; before one is drawn, the font's.
      const row = element.querySelector<HTMLElement>(".xterm-rows > div");
      const height = row?.getBoundingClientRect().height || rect.height * LINE_HEIGHT;
      return { width: rect.width / chars, height };
    };
    // The room inside the padding.
    const size = () => {
      const box = element.getBoundingClientRect();
      const style = getComputedStyle(element);
      const px = (value: string) => Number.parseFloat(value) || 0;
      return fit(
        {
          width: box.width - px(style.paddingLeft) - px(style.paddingRight),
          height: box.height - px(style.paddingTop) - px(style.paddingBottom),
        },
        cellSize(),
      );
    };
    const resize = () => {
      const next = size();
      if (!next || (next.cols === xterm.cols && next.rows === xterm.rows)) return;
      xterm.resize(next.cols, next.rows);
      if (id !== null && !exited) void port.resize(id, next.cols, next.rows);
    };
    const observer = new ResizeObserver(() => resize());
    observer.observe(element);

    const typed = xterm.onData(write);
    let live = true;
    // What's to be typed once the prompt is up: after the output pauses.
    let input = initialInput;
    let quiet: ReturnType<typeof setTimeout> | undefined;
    const typeWhenQuiet = () => {
      if (!input) return;
      clearTimeout(quiet);
      quiet = setTimeout(() => {
        if (!input || id === null || exited) return;
        void port.write(id, input);
        input = undefined;
      }, PROMPT_QUIET_MS);
    };
    const start = size() ?? { cols: 80, rows: 24 };
    xterm.resize(start.cols, start.rows);
    latest.current.onStatus({ kind: "starting" });
    void port
      .open(start.cols, start.rows, (event) => {
        if (event.kind === "output") {
          xterm.write(event.data);
          typeWhenQuiet();
        } else {
          exited = true;
          const code = event.code ?? null;
          xterm.write(
            `\r\n\x1b[2m[The shell exited${code === null ? "" : ` with code ${code}`}.]\x1b[0m\r\n`,
          );
          latest.current.onStatus({ kind: "exited", code });
        }
      })
      .then((result) => {
        if (!result.ok) {
          xterm.write(`\x1b[31mCouldn't start the shell: ${result.error.message}\x1b[0m\r\n`);
          latest.current.onStatus({ kind: "failed", message: result.error.message });
          return;
        }
        const { terminal, stop: stopEvents } = result.value;
        stop = stopEvents;
        if (!live) {
          // The view went while the shell was starting.
          stop();
          void port.close(terminal.id);
          return;
        }
        id = terminal.id;
        latest.current.onStatus({ kind: "running", id: terminal.id, shell: terminal.shell });
        // Its size may have changed while it started.
        resize();
      });
    // A panel that's opening can't take focus yet: try for a few frames.
    latest.current.onReady?.(() => {
      let frames = 0;
      const attempt = () => {
        xterm.focus();
        if (document.activeElement !== xterm.textarea && ++frames < 30) {
          requestAnimationFrame(attempt);
        }
      };
      attempt();
    });

    return () => {
      live = false;
      clearTimeout(quiet);
      observer.disconnect();
      typed.dispose();
      stop();
      if (id !== null) void port.close(id);
      xterm.dispose();
      term.current = null;
    };
  }, [port]);

  // Settings that can change while it runs.
  useEffect(() => {
    const xterm = term.current;
    if (xterm) xterm.options.screenReaderMode = screenReader;
  }, [screenReader]);
  // biome-ignore lint/correctness/useExhaustiveDependencies: `themeKey` changing is what asks for the colors to be read again
  useEffect(() => {
    const xterm = term.current;
    if (xterm && host.current) xterm.options.theme = terminalTheme(host.current);
  }, [themeKey]);
  useEffect(() => {
    term.current?.textarea?.setAttribute("aria-label", label);
  }, [label]);
  useEffect(() => {
    if (active) term.current?.refresh(0, (term.current?.rows ?? 1) - 1);
  }, [active]);

  return <div ref={host} className="terminal-view" />;
}
