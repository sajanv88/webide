/** Something that can be undone by calling it once, such as a registration. */
export type Dispose = () => void;

export interface Command<Id extends string> {
  readonly id: Id;
  /** What menus and the palette show. */
  readonly title: string;
  /** What it belongs to, such as "File" or "View": the palette groups by it and shows "File: Save". */
  readonly category?: string;
  /**
   * When it can run (VS Code's `when`). Asked every time a menu draws, the
   * palette opens, a key is pressed or the command runs. Enabled when absent.
   * Menus show a disabled command greyed out; the palette leaves it out.
   */
  readonly isEnabled?: () => boolean;
  /** `false` keeps it out of the palette, as for a command that needs arguments. */
  readonly palette?: boolean;
  /** Receives whatever the caller passed; parse it before use. */
  readonly run: (args: unknown) => void | Promise<void>;
}

export type ExecuteResult =
  | { readonly ok: true }
  | { readonly ok: false; readonly reason: "notRegistered" | "disabled" }
  | { readonly ok: false; readonly reason: "failed"; readonly error: unknown };

/**
 * The commands every module can run and contribute to. Menus, keybindings and
 * the palette are all built from it, so no list is written twice.
 */
export interface CommandRegistry<Id extends string> {
  /** Adds a command. A second command with the same id is a bug, and throws. */
  register(command: Command<Id>): Dispose;
  get(id: Id): Command<Id> | undefined;
  all(): readonly Command<Id>[];
  isEnabled(id: Id): boolean;
  /** Runs a command. Never throws: failures come back as a result, and go to `onError`. */
  execute(id: Id, args?: unknown): Promise<ExecuteResult>;
  /** Called after any registration change, or after `notify()`. Returns the unsubscribe. */
  subscribe(listener: () => void): Dispose;
  /** Tells subscribers that something `isEnabled` reads has changed. */
  notify(): void;
  /** Increases on every change, for `useSyncExternalStore`. */
  version(): number;
}

export interface CommandRegistryOptions<Id extends string> {
  readonly onError?: (id: Id, error: unknown) => void;
}

export function createCommandRegistry<Id extends string>(
  options: CommandRegistryOptions<Id> = {},
): CommandRegistry<Id> {
  const commands = new Map<Id, Command<Id>>();
  const listeners = new Set<() => void>();
  let version = 0;

  const changed = () => {
    version += 1;
    for (const listener of [...listeners]) listener();
  };

  const isEnabled = (id: Id): boolean => {
    const command = commands.get(id);
    return command !== undefined && (command.isEnabled?.() ?? true);
  };

  return {
    register(command) {
      if (commands.has(command.id)) {
        throw new Error(`Command "${command.id}" is already registered`);
      }
      commands.set(command.id, command);
      changed();
      return () => {
        if (commands.get(command.id) === command) {
          commands.delete(command.id);
          changed();
        }
      };
    },
    get: (id) => commands.get(id),
    all: () => [...commands.values()],
    isEnabled,
    async execute(id, args) {
      const command = commands.get(id);
      if (!command) return { ok: false, reason: "notRegistered" };
      if (!isEnabled(id)) return { ok: false, reason: "disabled" };
      try {
        await command.run(args);
        return { ok: true };
      } catch (error: unknown) {
        options.onError?.(id, error);
        return { ok: false, reason: "failed", error };
      }
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    notify: changed,
    version: () => version,
  };
}
