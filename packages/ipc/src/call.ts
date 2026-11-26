import type { CommandMap, CommandName, IpcError, IpcErrorCode, IpcResult } from "@repo/contracts";
import { invoke } from "@tauri-apps/api/core";

export type { IpcResult };

type Args<K extends CommandName> = CommandMap[K]["args"];
type Output<K extends CommandName> = CommandMap[K]["output"];
type ArgsParam<K extends CommandName> = Args<K> extends null ? [] : [args: Args<K>];

/**
 * Calls a command by name, with the argument and result types from the Rust side.
 * Commands that take input receive it as `{ args }` (see crates/ide-ipc/src/commands.rs).
 */
export async function call<K extends CommandName>(
  name: K,
  ...input: ArgsParam<K>
): Promise<IpcResult<Output<K>>> {
  try {
    const value =
      input.length === 0
        ? await invoke<Output<K>>(name)
        : await invoke<Output<K>>(name, { args: input[0] });
    return { ok: true, value };
  } catch (error: unknown) {
    return { ok: false, error: toIpcError(error) };
  }
}

// A key for every code: adding one on the Rust side fails typecheck here until listed.
const CODES: Readonly<Record<IpcErrorCode, true>> = {
  invalidPath: true,
  outsideWorkspace: true,
  notFound: true,
  notADirectory: true,
  notAFile: true,
  permissionDenied: true,
  noWorkspace: true,
  workspaceChanged: true,
  notARecentWorkspace: true,
  invalidSettings: true,
  invalidQuery: true,
  restricted: true,
  git: true,
  invalidArgument: true,
  io: true,
  internal: true,
};

function isCode(value: unknown): value is IpcErrorCode {
  return typeof value === "string" && Object.hasOwn(CODES, value);
}

/**
 * A rejection as an `IpcError`. Commands reject with one already; anything else
 * (Tauri refusing the arguments, a permission denial, a crash) becomes `internal`.
 */
export function toIpcError(error: unknown): IpcError {
  if (typeof error === "object" && error !== null) {
    const { code, message } = error as { code?: unknown; message?: unknown };
    if (isCode(code) && typeof message === "string") {
      return { code, message };
    }
  }
  return { code: "internal", message: error instanceof Error ? error.message : String(error) };
}
