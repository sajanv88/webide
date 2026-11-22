// Hand-written, frontend-only contracts: how modules talk to each other.
// (The IPC types are generated from Rust, in ./generated.)
import type {
  DirListingDto,
  FileChangeDto,
  FileStampDto,
  IpcError,
  ReadFileDto,
  SettingsDto,
  SettingsPatchDto,
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
  | "view.toggleExplorer"
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
