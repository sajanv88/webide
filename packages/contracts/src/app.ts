// Hand-written, frontend-only contracts: how modules talk to each other.
// (The IPC types are generated from Rust, in ./generated.)
import type {
  DirListingDto,
  FileChangeDto,
  FileListDto,
  FileStampDto,
  GitBranchDto,
  GitDiffDto,
  GitStateDto,
  IpcError,
  ReadFileDto,
  SearchTextArgs,
  SettingsDto,
  SettingsPatchDto,
  TextSearchDto,
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
  | "file.save"
  | "file.saveAll"
  | "tabs.close"
  | "tabs.closeOthers"
  | "tabs.closeAll"
  | "tabs.next"
  | "tabs.previous"
  | "commands.showPalette"
  | "settings.open"
  | "help.about";

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
