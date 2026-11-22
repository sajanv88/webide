import {
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandShortcut,
} from "@nuvui/react";
import type { AppCommandId } from "@repo/contracts";
import type { Command, CommandRegistry } from "@repo/kernel";
import { useEffect, useState, useSyncExternalStore } from "react";
import type { Keybindings } from "./keybindings";

export interface CommandPaletteProps {
  readonly registry: CommandRegistry<AppCommandId>;
  /** Shown next to each command, and announced with it. */
  readonly keybindings: Keybindings<AppCommandId>;
}

/** Commands without a category go last, under this heading. */
const OTHER = "Other";

/**
 * What the palette lists: commands that can run now, grouped by category.
 * Categories are in alphabetical order ("Other" last), so the order doesn't
 * depend on which module happened to register first; commands keep theirs.
 */
export function paletteGroups(
  commands: readonly Command<AppCommandId>[],
  isEnabled: (id: AppCommandId) => boolean,
): [string, Command<AppCommandId>[]][] {
  const groups = new Map<string, Command<AppCommandId>[]>();
  for (const command of commands) {
    if (command.palette === false || !isEnabled(command.id)) continue;
    const category = command.category ?? OTHER;
    const group = groups.get(category) ?? [];
    group.push(command);
    groups.set(category, group);
  }
  return [...groups].sort(([a], [b]) => (a === OTHER ? 1 : b === OTHER ? -1 : a.localeCompare(b)));
}

/**
 * The command palette, on nuvui `CommandDialog`: every command that can run
 * now, from the registry, searchable, with its keys. It registers
 * `commands.showPalette`, which opens it.
 */
export function CommandPalette({ registry, keybindings }: CommandPaletteProps) {
  const [open, setOpen] = useState(false);
  useSyncExternalStore(registry.subscribe, registry.version);

  useEffect(
    () =>
      registry.register({
        id: "commands.showPalette",
        title: "Command Palette…",
        category: "View",
        palette: false,
        run: () => setOpen(true),
      }),
    [registry],
  );

  const run = (id: AppCommandId) => {
    // Closed first: a command that changes the page shouldn't happen behind it.
    setOpen(false);
    void registry.execute(id);
  };

  const groups = open ? paletteGroups(registry.all(), registry.isEnabled) : [];
  return (
    <CommandDialog
      open={open}
      onOpenChange={setOpen}
      title="Command palette"
      label="Search commands"
      loop
    >
      <CommandInput placeholder="Type the name of a command" />
      <CommandList label="Commands">
        <CommandEmpty>No command matches.</CommandEmpty>
        {groups.map(([category, commands]) => (
          <CommandGroup key={category} heading={category}>
            {commands.map((command) => {
              const binding = keybindings.forCommand(command.id);
              return (
                <CommandItem
                  key={command.id}
                  // What the search matches, and unique: "File: Save".
                  value={`${category}: ${command.title}`}
                  keywords={[command.id]}
                  onSelect={() => run(command.id)}
                  {...(binding ? { "aria-keyshortcuts": binding.aria } : {})}
                >
                  {command.title}
                  {binding ? <CommandShortcut>{binding.label}</CommandShortcut> : null}
                </CommandItem>
              );
            })}
          </CommandGroup>
        ))}
      </CommandList>
    </CommandDialog>
  );
}
