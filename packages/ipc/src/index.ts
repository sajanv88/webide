// Public entry of @repo/ipc: the only package that imports @tauri-apps/api.
import type {
  AppInfoDto,
  CompletionsDto,
  DefinitionDto,
  DirListingDto,
  FileListDto,
  FileStampDto,
  GitBranchDto,
  GitCommitArgs,
  GitDiffArgs,
  GitDiffDto,
  GitPathsArgs,
  GitStateDto,
  GitSwitchArgs,
  HoverDto,
  LspDocumentArgs,
  LspPositionArgs,
  OpenRecentArgs,
  ProblemDto,
  ReadFileDto,
  RecentWorkspaceDto,
  SearchTextArgs,
  SettingsDto,
  SettingsPatchDto,
  TextSearchDto,
  WorkspaceArgs,
  WorkspaceDto,
  WorkspacePathArgs,
  WriteFileArgs,
} from "@repo/contracts";
import { call, type IpcResult } from "./call";

export { call, type IpcResult, toIpcError } from "./call";
export { closeWindow, setCloseGuard, subscribeEvents } from "./events";
export { getStyleNonce } from "./nonce";
export { closeTerminal, openTerminal, resizeTerminal, writeTerminal } from "./terminal";

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

/** Find in files; a new search cancels the one still running. */
export const searchText = (args: SearchTextArgs): Promise<IpcResult<TextSearchDto>> =>
  call("search_text", args);

/** The workspace's files, for quick open (ignored ones left out). */
export const listFiles = (args: WorkspaceArgs): Promise<IpcResult<FileListDto>> =>
  call("list_files", args);

export const gitState = (args: WorkspaceArgs): Promise<IpcResult<GitStateDto>> =>
  call("git_state", args);

export const gitDiff = (args: GitDiffArgs): Promise<IpcResult<GitDiffDto>> =>
  call("git_diff", args);

export const gitStage = (args: GitPathsArgs): Promise<IpcResult<null>> => call("git_stage", args);

export const gitUnstage = (args: GitPathsArgs): Promise<IpcResult<null>> =>
  call("git_unstage", args);

export const gitDiscard = (args: GitPathsArgs): Promise<IpcResult<null>> =>
  call("git_discard", args);

export const gitCommit = (args: GitCommitArgs): Promise<IpcResult<null>> =>
  call("git_commit", args);

export const gitBranches = (args: WorkspaceArgs): Promise<IpcResult<GitBranchDto[]>> =>
  call("git_branches", args);

export const gitSwitchBranch = (args: GitSwitchArgs): Promise<IpcResult<null>> =>
  call("git_switch_branch", args);

export const gitInit = (args: WorkspaceArgs): Promise<IpcResult<null>> => call("git_init", args);

export const lspOpen = (args: LspDocumentArgs): Promise<IpcResult<null>> => call("lsp_open", args);

export const lspChange = (args: LspDocumentArgs): Promise<IpcResult<null>> =>
  call("lsp_change", args);

export const lspClose = (args: WorkspacePathArgs): Promise<IpcResult<null>> =>
  call("lsp_close", args);

export const lspProblems = (args: WorkspacePathArgs): Promise<IpcResult<ProblemDto[]>> =>
  call("lsp_problems", args);

export const lspCompletions = (args: LspPositionArgs): Promise<IpcResult<CompletionsDto>> =>
  call("lsp_completions", args);

export const lspHover = (args: LspPositionArgs): Promise<IpcResult<HoverDto | null>> =>
  call("lsp_hover", args);

export const lspDefinition = (args: LspPositionArgs): Promise<IpcResult<DefinitionDto[]>> =>
  call("lsp_definition", args);

export const lspRestart = (args: WorkspaceArgs): Promise<IpcResult<null>> =>
  call("lsp_restart", args);
