// Hand-written, frontend-only contracts: how modules talk to each other.
// (The IPC types are generated from Rust, in ./generated.)
import type {
  AgentCallDto,
  AgentConfigArgs,
  AgentStatusDto,
  AssistantDecideArgs,
  AssistantEventDto,
  AssistantSendArgs,
  AssistantSetupArgs,
  AssistantStatusDto,
  BreakpointDto,
  CodeActionDto,
  CompletionsDto,
  DebugEventDto,
  DefinitionDto,
  DirListingDto,
  FileChangeDto,
  FileListDto,
  FileStampDto,
  GitBranchDto,
  GitDiffDto,
  GitStateDto,
  HoverDto,
  IpcError,
  LanguageStatusDto,
  ModelInfoDto,
  ProblemDto,
  ReadFileDto,
  ResolvedCompletionDto,
  ScopeDto,
  SearchTextArgs,
  SettingsDto,
  SettingsPatchDto,
  SourceActionDto,
  StackFrameDto,
  TerminalDto,
  TerminalEventDto,
  TextEditDto,
  TextPositionDto,
  TextSearchDto,
  VariableDto,
  WorkspaceDto,
} from "./generated/index";

/** What a port call resolves to. Expected failures are values, not exceptions. */
export type IpcResult<T> =
  | { readonly ok: true; readonly value: T }
  | { readonly ok: false; readonly error: IpcError };

/**
 * Every command id in the app's registry. Modules register and run commands by
 * these ids only, so a typo is a type error.
 */
export type AppCommandId =
  | "workspace.openFolder"
  | "workspace.openRecent"
  | "explorer.refresh"
  | "explorer.collapseAll"
  | "view.toggleSidebar"
  | "view.showView"
  | "view.showExplorer"
  | "search.findInFiles"
  | "search.goToFile"
  | "view.showSourceControl"
  | "git.refresh"
  | "git.commit"
  | "git.stageAll"
  | "git.unstageAll"
  | "git.switchBranch"
  | "git.init"
  | "view.togglePanel"
  | "view.showPanel"
  | "terminal.new"
  | "terminal.kill"
  | "terminal.focus"
  | "terminal.screenReader"
  | "language.restart"
  | "editor.quickFix"
  | "editor.organizeImports"
  | "editor.removeUnusedImports"
  | "editor.sortImports"
  | "editor.fixAll"
  | "view.showDebug"
  | "debug.start"
  | "debug.stop"
  | "debug.continue"
  | "debug.pause"
  | "debug.stepOver"
  | "debug.stepInto"
  | "debug.stepOut"
  | "file.save"
  | "file.saveAll"
  | "tabs.close"
  | "tabs.closeOthers"
  | "tabs.closeAll"
  | "tabs.next"
  | "tabs.previous"
  | "commands.showPalette"
  | "settings.open"
  | "agents.open"
  | "view.toggleAside"
  | "view.showAside"
  | "assistant.focus"
  | "assistant.setup"
  | "assistant.newChat"
  | "help.about"
  | "app.installCommandLine"
  | "app.uninstallCommandLine"
  | "workspace.manageTrust";

/** Events on the app's bus, by name, with their payloads. */
export interface AppEvents {
  /** A workspace was opened (picked, or reopened from the recent list). */
  "workspace.opened": WorkspaceDto;
  /** A file was chosen in the explorer. */
  "explorer.fileActivated": { readonly path: string };
  /**
   * Open a file (from search, quick open…), and put the cursor at `line` and
   * `column` (1-based, UTF-16 units) when given. The editor takes focus.
   */
  "file.openRequested": {
    readonly path: string;
    readonly line?: number;
    readonly column?: number;
  };
  /**
   * Files of the open workspace changed on disk, by anyone (this app's own
   * saves included). With `rescan`, `changes` is incomplete: reload whatever
   * is shown.
   */
  "workspace.filesChanged": {
    readonly changes: readonly FileChangeDto[];
    readonly rescan: boolean;
  };
  /** The settings were saved; this is what they are now. */
  "settings.changed": SettingsDto;
  /** A file's text is open in the editor (once its text has been read). */
  "document.opened": { readonly path: string; readonly text: string };
  /** An open file's text changed: typed, or reloaded from disk. */
  "document.changed": { readonly path: string; readonly text: string };
  /** A file was closed in the editor. */
  "document.closed": { readonly path: string };
  /** The problems the language server found in an open file, now. */
  "language.problems": { readonly path: string; readonly problems: readonly EditorProblem[] };
  /** How one of the open workspace's language servers ("TypeScript", "JSON"…) is doing. */
  "language.status": { readonly server: string; readonly status: LanguageStatusDto };
  /**
   * Opens a new terminal with `text` typed at its prompt, not run: the
   * person reads it and presses Enter (an installer's command, say).
   */
  "terminal.type": { readonly text: string };
  /** What the program running under the debugger reports. */
  "debug.event": DebugEventDto;
  /** An AI agent used a tool of the MCP server. */
  "agent.call": AgentCallDto;
  /** What a conversation with the IDE's assistant reports. */
  "assistant.event": AssistantEventDto;
}

/** What the explorer needs from the outside: one level of a folder in the open workspace. */
export interface ExplorerPort {
  /** `path` is workspace-relative, `/`-separated; `""` is the root. */
  listDirectory(path: string): Promise<IpcResult<DirListingDto>>;
}

/**
 * What open documents need from the outside: a file of the open workspace, read
 * and written. `path` is workspace-relative, `/`-separated.
 */
export interface DocumentsPort {
  readFile(path: string): Promise<IpcResult<ReadFileDto>>;
  /** Writes `text` as it is: line endings are the caller's to choose. */
  writeFile(path: string, text: string): Promise<IpcResult<FileStampDto>>;
}

/** What the settings screen needs: the user's settings, read and saved. */
export interface SettingsPort {
  get(): Promise<IpcResult<SettingsDto>>;
  /** Saves the fields given; the rest stay. Resolves to the settings as saved. */
  update(patch: SettingsPatchDto): Promise<IpcResult<SettingsDto>>;
}

/** What search needs: find in files, and the file list for quick open, in the open workspace. */
export interface SearchPort {
  /** A new search cancels the one still running (its result says `cancelled`). */
  searchText(query: Omit<SearchTextArgs, "workspaceId">): Promise<IpcResult<TextSearchDto>>;
  listFiles(): Promise<IpcResult<FileListDto>>;
}

/**
 * Trusting the open folder's authors. `parent` trusts the folder it's in
 * (and so every folder there) instead. Resolves to the workspace, with its
 * trust as it is now.
 */
export interface TrustPort {
  setTrust(trusted: boolean, parent: boolean): Promise<IpcResult<WorkspaceDto>>;
}

/**
 * What source control needs: git in the open workspace. Paths are
 * workspace-relative. A refusal from git is an `IpcError` with code `git` and
 * git's own words as its message.
 */
export interface GitPort {
  state(): Promise<IpcResult<GitStateDto>>;
  /** The two versions of a file: `staged` (last commit → index) or `unstaged` (index → working tree). */
  diff(path: string, side: "staged" | "unstaged"): Promise<IpcResult<GitDiffDto>>;
  stage(paths: readonly string[]): Promise<IpcResult<null>>;
  unstage(paths: readonly string[]): Promise<IpcResult<null>>;
  /** Puts the files back as they are in the index: their unstaged changes are lost. */
  discard(paths: readonly string[]): Promise<IpcResult<null>>;
  commit(message: string): Promise<IpcResult<null>>;
  branches(): Promise<IpcResult<GitBranchDto[]>>;
  switchBranch(name: string, create: boolean): Promise<IpcResult<null>>;
  init(): Promise<IpcResult<null>>;
}

/** A terminal the app started, and how to stop hearing from it. */
export interface OpenTerminal {
  readonly terminal: TerminalDto;
  /** Stops events from reaching `onEvent`. (Closing the terminal is `close`.) */
  readonly stop: () => void;
}

/**
 * What terminals need: the person's shell in the open workspace's folder.
 * The program, its arguments and its folder are the backend's to choose.
 */
export interface TerminalPort {
  /** Starts a shell `cols` × `rows` cells; its output and its exit go to `onEvent`. */
  open(
    cols: number,
    rows: number,
    onEvent: (event: TerminalEventDto) => void,
  ): Promise<IpcResult<OpenTerminal>>;
  /** Keystrokes or pasted text. */
  write(id: number, data: string): Promise<IpcResult<null>>;
  resize(id: number, cols: number, rows: number): Promise<IpcResult<null>>;
  /** Ends the shell. */
  close(id: number): Promise<IpcResult<null>>;
}

/** A problem in a text, placed by character offsets (for the editor) and by line. */
export interface EditorProblem {
  /** Characters (UTF-16 units) from the start of the text. */
  readonly from: number;
  readonly to: number;
  readonly severity: "error" | "warning" | "info" | "hint";
  readonly message: string;
  readonly source?: string;
  /** 1-based, for lists. */
  readonly line: number;
  readonly column: number;
}

/**
 * What language features need: the open workspace's language server.
 * Positions are 0-based lines and UTF-16 columns.
 */
export interface LanguagePort {
  open(path: string, text: string): Promise<IpcResult<null>>;
  change(path: string, text: string): Promise<IpcResult<null>>;
  close(path: string): Promise<IpcResult<null>>;
  problems(path: string): Promise<IpcResult<ProblemDto[]>>;
  completions(path: string, line: number, character: number): Promise<IpcResult<CompletionsDto>>;
  hover(path: string, line: number, character: number): Promise<IpcResult<HoverDto | null>>;
  definition(path: string, line: number, character: number): Promise<IpcResult<DefinitionDto[]>>;
  /** What else picking a completion does (the import it adds), by its `resolve`. */
  resolveCompletion(path: string, resolve: string): Promise<IpcResult<ResolvedCompletionDto>>;
  /** The fixes for the problems between `start` and `end`. */
  fixes(
    path: string,
    start: TextPositionDto,
    end: TextPositionDto,
  ): Promise<IpcResult<CodeActionDto[]>>;
  /** The edits that organize the imports (or make another source action); `null` when not offered. */
  sourceAction(path: string, action: SourceActionDto): Promise<IpcResult<TextEditDto[] | null>>;
  /** Stops the server; it starts again when it's next needed. */
  restart(): Promise<IpcResult<null>>;
}

/** What debugging needs: one program at a time, under the debugger, in the open workspace. */
export interface DebugPort {
  /** Runs `path`, with breakpoints set first (1-based lines). */
  start(
    path: string,
    breakpoints: readonly { path: string; lines: readonly number[] }[],
  ): Promise<IpcResult<null>>;
  stop(): Promise<IpcResult<null>>;
  setBreakpoints(path: string, lines: readonly number[]): Promise<IpcResult<BreakpointDto[]>>;
  /** Continues, or steps when `step` says how. */
  resume(threadId: number, step?: "over" | "into" | "out"): Promise<IpcResult<null>>;
  pause(threadId: number): Promise<IpcResult<null>>;
  stack(threadId: number): Promise<IpcResult<StackFrameDto[]>>;
  scopes(frameId: number): Promise<IpcResult<ScopeDto[]>>;
  variables(reference: number): Promise<IpcResult<VariableDto[]>>;
  evaluate(expression: string, frameId?: number): Promise<IpcResult<string>>;
}

/** The MCP server that lets AI agents work in the open workspace. */
export interface AgentPort {
  status(): Promise<IpcResult<AgentStatusDto>>;
  /** Saves the settings, and starts, stops or restarts the server to match. */
  configure(config: AgentConfigArgs): Promise<IpcResult<AgentStatusDto>>;
  /** Replaces the token: agents with the old one are refused from then on. */
  newToken(): Promise<IpcResult<AgentStatusDto>>;
}

/**
 * The IDE's AI assistant: its setup (the key goes to the system's
 * credential store, never back), and conversations, whose replies arrive as
 * `assistant.event`.
 */
export interface AssistantPort {
  status(): Promise<IpcResult<AssistantStatusDto>>;
  save(args: AssistantSetupArgs): Promise<IpcResult<AssistantStatusDto>>;
  /** Forgets the setup and every key kept. */
  forget(): Promise<IpcResult<AssistantStatusDto>>;
  /** The models the provider offers. */
  models(args: AssistantSetupArgs): Promise<IpcResult<ModelInfoDto[]>>;
  /** Asks the model for a word: its answer, when the setup works. */
  check(args: AssistantSetupArgs): Promise<IpcResult<string>>;
  send(args: AssistantSendArgs): Promise<IpcResult<null>>;
  stop(conversationId: string): Promise<IpcResult<null>>;
  /** Accepts (writes) or rejects a change it proposed. */
  decide(args: AssistantDecideArgs): Promise<IpcResult<null>>;
  /** Forgets a conversation. */
  clear(conversationId: string): Promise<IpcResult<null>>;
}
