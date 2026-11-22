// Public entry of @repo/ipc: the only package that imports @tauri-apps/api.
import type {
  AppInfoDto,
  DirListingDto,
  FileStampDto,
  OpenRecentArgs,
  ReadFileDto,
  RecentWorkspaceDto,
  SettingsDto,
  SettingsPatchDto,
  WorkspaceDto,
  WorkspacePathArgs,
  WriteFileArgs,
} from "@repo/contracts";
import { call, type IpcResult } from "./call";

export { call, type IpcResult, toIpcError } from "./call";
export { closeWindow, setCloseGuard, subscribeEvents } from "./events";
export { getStyleNonce } from "./nonce";

/** The app's name and version, from tauri.conf.json (the one place the name lives). */
export const appInfo = (): Promise<IpcResult<AppInfoDto>> => call("app_info");

/** Shows the native folder picker and opens what's chosen; `null` when cancelled. */
export const pickWorkspace = (): Promise<IpcResult<WorkspaceDto | null>> => call("pick_workspace");

/** Opens a folder from `recentWorkspaces()`. No other folder can be opened by path. */
export const openRecentWorkspace = (args: OpenRecentArgs): Promise<IpcResult<WorkspaceDto>> =>
  call("open_recent_workspace", args);

export const recentWorkspaces = (): Promise<IpcResult<RecentWorkspaceDto[]>> =>
  call("recent_workspaces");

/** The open workspace, if any (it survives a reload of the page). */
export const currentWorkspace = (): Promise<IpcResult<WorkspaceDto | null>> =>
  call("current_workspace");

/** One level of a folder; `path` is workspace-relative, `""` for the root. */
export const listDirectory = (args: WorkspacePathArgs): Promise<IpcResult<DirListingDto>> =>
  call("list_directory", args);

export const readFile = (args: WorkspacePathArgs): Promise<IpcResult<ReadFileDto>> =>
  call("read_file", args);

export const writeFile = (args: WriteFileArgs): Promise<IpcResult<FileStampDto>> =>
  call("write_file", args);

export const getSettings = (): Promise<IpcResult<SettingsDto>> => call("get_settings");

export const updateSettings = (patch: SettingsPatchDto): Promise<IpcResult<SettingsDto>> =>
  call("update_settings", patch);
