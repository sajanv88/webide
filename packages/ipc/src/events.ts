import type { BackendEventDto } from "@repo/contracts";
import { Channel } from "@tauri-apps/api/core";
import { call, type IpcResult } from "./call";

/**
 * Hears what the backend sends this page: file changes, and the request to
 * close the window. One subscription per page: a second replaces the first on
 * the backend. Resolves to the function that stops listening.
 */
export async function subscribeEvents(
  onEvent: (event: BackendEventDto) => void,
): Promise<IpcResult<() => void>> {
  const channel = new Channel<BackendEventDto>(onEvent);
  const result = await call("subscribe_events", { channel });
  // A replaced channel gets nothing more from the backend; this also drops
  // anything already on its way.
  const stop = () => {
    channel.onmessage = () => {};
  };
  if (!result.ok) {
    stop();
    return result;
  }
  return { ok: true, value: stop };
}

/**
 * Says whether closing the window now would lose unsaved changes. While it
 * would, closing sends `closeRequested` instead, and the window stays open
 * until `closeWindow()`.
 */
export const setCloseGuard = (unsaved: boolean): Promise<IpcResult<null>> =>
  call("set_close_guard", { unsaved });

/** Closes the window without asking again. */
export const closeWindow = (): Promise<IpcResult<null>> => call("close_window");
