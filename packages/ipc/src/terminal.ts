import type { OpenTerminal, TerminalEventDto } from "@repo/contracts";
import { Channel } from "@tauri-apps/api/core";
import { call, type IpcResult } from "./call";

/**
 * Starts the person's shell in the open workspace's folder, `cols` × `rows`
 * cells. Its output and its exit arrive at `onEvent`, over a channel of its
 * own.
 */
export async function openTerminal(
  args: { readonly workspaceId: string; readonly cols: number; readonly rows: number },
  onEvent: (event: TerminalEventDto) => void,
): Promise<IpcResult<OpenTerminal>> {
  const channel = new Channel<TerminalEventDto>(onEvent);
  const stop = () => {
    channel.onmessage = () => {};
  };
  const result = await call("terminal_open", { ...args, channel });
  if (!result.ok) {
    stop();
    return result;
  }
  return { ok: true, value: { terminal: result.value, stop } };
}

export const writeTerminal = (id: number, data: string): Promise<IpcResult<null>> =>
  call("terminal_write", { id, data });

export const resizeTerminal = (id: number, cols: number, rows: number): Promise<IpcResult<null>> =>
  call("terminal_resize", { id, cols, rows });

export const closeTerminal = (id: number): Promise<IpcResult<null>> =>
  call("terminal_close", { id });
