import "./window-controls.scss";
import type { MouseEvent as ReactMouseEvent } from "react";

/**
 * The window the shell draws the frame of: its system, how it's shown, and
 * what its own buttons do. The app has no system title bar on Windows and
 * Linux; on macOS the system's buttons stay, over the shell's bar.
 */
export interface WindowFrame {
  readonly os: "windows" | "macos" | "linux";
  readonly maximized: boolean;
  readonly fullscreen: boolean;
  readonly minimize: () => void;
  readonly toggleMaximize: () => void;
  /** Asks the window to close, as its system button would. */
  readonly close: () => void;
  /** Moves the window with the pointer, from a press on the title bar. */
  readonly startDrag: () => void;
}

/** What in the bar takes a press itself, rather than moving the window. */
const INTERACTIVE = "button, a, input, select, textarea, [role='menubar'], [role='menuitem']";

/**
 * The title bar's empty parts move the window, and a double click on them
 * maximizes or restores it, as a system title bar does.
 */
export function dragHandlers(frame: WindowFrame | undefined) {
  if (!frame) return {};
  const onBar = (event: ReactMouseEvent<HTMLElement>) =>
    event.button === 0 &&
    !(event.target instanceof Element && event.target.closest(INTERACTIVE) !== null);
  return {
    onMouseDown: (event: ReactMouseEvent<HTMLElement>) => {
      // The second press of a double click mustn't start a move.
      if (onBar(event) && event.detail === 1) frame.startDrag();
    },
    onDoubleClick: (event: ReactMouseEvent<HTMLElement>) => {
      if (onBar(event)) frame.toggleMaximize();
    },
  };
}

// The buttons' glyphs, drawn at 10 by 10 like the system's.
const MINIMIZE = "M0 5.5h10";
const MAXIMIZE = "M0.5 0.5h9v9h-9z";
const RESTORE = "M2.5 2.5h7v7h-7z M2.5 2.5v-2h7v7h-2";
const CLOSE = "M0.5 0.5l9 9 M9.5 0.5l-9 9";

function Glyph({ path }: { readonly path: string }) {
  return (
    <svg
      className="window-controls__glyph"
      viewBox="0 0 10 10"
      width="10"
      height="10"
      aria-hidden="true"
      focusable="false"
    >
      <path d={path} />
    </svg>
  );
}

/**
 * Minimize, maximize or restore, and close, at the end of the title bar, as
 * Windows and Linux draw them. On macOS the system's own buttons are at the
 * start instead, and this draws nothing.
 */
export function WindowControls({ frame }: { readonly frame: WindowFrame }) {
  if (frame.os === "macos" || frame.fullscreen) return null;
  return (
    <div className="window-controls">
      <button
        type="button"
        className="window-controls__button"
        aria-label="Minimize"
        title="Minimize"
        tabIndex={-1}
        onClick={frame.minimize}
      >
        <Glyph path={MINIMIZE} />
      </button>
      <button
        type="button"
        className="window-controls__button"
        aria-label={frame.maximized ? "Restore" : "Maximize"}
        title={frame.maximized ? "Restore" : "Maximize"}
        tabIndex={-1}
        onClick={frame.toggleMaximize}
      >
        <Glyph path={frame.maximized ? RESTORE : MAXIMIZE} />
      </button>
      <button
        type="button"
        className="window-controls__button window-controls__button--close"
        aria-label="Close"
        title="Close"
        tabIndex={-1}
        onClick={frame.close}
      >
        <Glyph path={CLOSE} />
      </button>
    </div>
  );
}
