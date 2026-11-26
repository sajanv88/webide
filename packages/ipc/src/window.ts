import type { WindowStateDto } from "@repo/contracts";
import { call, type IpcResult } from "./call";

// The window's own buttons and title bar, which the page draws.

export const minimizeWindow = (): Promise<IpcResult<null>> => call("window_minimize");

/** Maximizes the window, or restores it when it's maximized. */
export const toggleMaximizeWindow = (): Promise<IpcResult<null>> => call("window_toggle_maximize");

/** Moves the window with the pointer: call it from a press on the title bar. */
export const dragWindow = (): Promise<IpcResult<null>> => call("window_drag");

/**
 * Asks the window to close, as its system button would: with unsaved
 * changes, `closeRequested` comes first.
 */
export const requestCloseWindow = (): Promise<IpcResult<null>> => call("window_close");

/** Whether the window is maximized or full screen now; `windowState` events follow it. */
export const windowState = (): Promise<IpcResult<WindowStateDto>> => call("window_state");
