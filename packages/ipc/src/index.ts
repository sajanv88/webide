// Public entry of @repo/ipc: the only package that imports @tauri-apps/api.
import type {
  AgentConfigArgs,
  AgentStatusDto,
  AppInfoDto,
  AssistantDecideArgs,
  AssistantSendArgs,
  AssistantSetupArgs,
  AssistantStatusDto,
  BreakpointDto,
  CompletionsDto,
  DebugBreakpointsArgs,
  DebugEvaluateArgs,
  DebugReferenceArgs,
  DebugStartArgs,
  DebugThreadArgs,
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
  ModelInfoDto,
  OpenRecentArgs,
  ProblemDto,
  ReadFileDto,
  RecentWorkspaceDto,
  ScopeDto,
  SearchTextArgs,
  SettingsDto,
  SettingsPatchDto,
  StackFrameDto,
  TextSearchDto,
  VariableDto,
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

export const debugStart = (args: DebugStartArgs): Promise<IpcResult<null>> =>
  call("debug_start", args);

export const debugStop = (args: WorkspaceArgs): Promise<IpcResult<null>> =>
  call("debug_stop", args);

export const debugBreakpoints = (args: DebugBreakpointsArgs): Promise<IpcResult<BreakpointDto[]>> =>
  call("debug_breakpoints", args);

export const debugResume = (args: DebugThreadArgs): Promise<IpcResult<null>> =>
  call("debug_resume", args);

export const debugPause = (args: DebugThreadArgs): Promise<IpcResult<null>> =>
  call("debug_pause", args);

export const debugStack = (args: DebugThreadArgs): Promise<IpcResult<StackFrameDto[]>> =>
  call("debug_stack", args);

export const debugScopes = (args: DebugReferenceArgs): Promise<IpcResult<ScopeDto[]>> =>
  call("debug_scopes", args);

export const debugVariables = (args: DebugReferenceArgs): Promise<IpcResult<VariableDto[]>> =>
  call("debug_variables", args);

export const debugEvaluate = (args: DebugEvaluateArgs): Promise<IpcResult<string>> =>
  call("debug_evaluate", args);

export const agentStatus = (): Promise<IpcResult<AgentStatusDto>> => call("agent_status");

export const agentConfigure = (args: AgentConfigArgs): Promise<IpcResult<AgentStatusDto>> =>
  call("agent_configure", args);

export const agentNewToken = (): Promise<IpcResult<AgentStatusDto>> => call("agent_new_token");

// The AI assistant. Its key goes in with `assistantSave` and never comes back.
export const assistantStatus = (): Promise<IpcResult<AssistantStatusDto>> =>
  call("assistant_status");

export const assistantSave = (args: AssistantSetupArgs): Promise<IpcResult<AssistantStatusDto>> =>
  call("assistant_save", args);

export const assistantForget = (): Promise<IpcResult<AssistantStatusDto>> =>
  call("assistant_forget");

export const assistantModels = (args: AssistantSetupArgs): Promise<IpcResult<ModelInfoDto[]>> =>
  call("assistant_models", args);

export const assistantCheck = (args: AssistantSetupArgs): Promise<IpcResult<string>> =>
  call("assistant_check", args);

export const assistantSend = (args: AssistantSendArgs): Promise<IpcResult<null>> =>
  call("assistant_send", args);

export const assistantStop = (conversationId: string): Promise<IpcResult<null>> =>
  call("assistant_stop", { conversationId });

export const assistantDecide = (args: AssistantDecideArgs): Promise<IpcResult<null>> =>
  call("assistant_decide", args);

export const assistantClear = (conversationId: string): Promise<IpcResult<null>> =>
  call("assistant_clear", { conversationId });
