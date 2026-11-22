import {
  Menubar,
  MenubarContent,
  MenubarItem,
  MenubarMenu,
  MenubarSeparator,
  MenubarShortcut,
  MenubarSub,
  MenubarSubContent,
  MenubarSubTrigger,
  MenubarTrigger,
} from "@nuvui/react";
import type { AppCommandId } from "@repo/contracts";
import type { CommandRegistry } from "@repo/kernel";
import { type ReactNode, useSyncExternalStore } from "react";

/** One row of a menu. Commands are named by id; their titles and state come from the registry. */
export type MenuItemSpec =
  | {
      readonly kind: "command";
      readonly command: AppCommandId;
      /** Overrides the command's title, as for each entry of "Open Recent". */
      readonly label?: string;
      readonly args?: unknown;
    }
  | { readonly kind: "separator" }
  | {
      readonly kind: "submenu";
      readonly label: string;
      /** A submenu with no items is shown disabled. */
      readonly items: readonly MenuItemSpec[];
    };

export interface MenuSpec {
  readonly label: string;
  readonly items: readonly MenuItemSpec[];
}

/** The keys bound to a command, as menus show them and as `aria-keyshortcuts` writes them. */
export interface MenuShortcut {
  readonly label: string;
  readonly aria: string;
}

interface AppMenubarProps {
  readonly registry: CommandRegistry<AppCommandId>;
  readonly menus: readonly MenuSpec[];
  readonly shortcutFor?: ((command: AppCommandId) => MenuShortcut | undefined) | undefined;
}

/**
 * The items to draw: commands nobody has registered are left out (a module
 * that isn't shown, such as the explorer with no folder open), and so are the
 * separators that would then lead, trail or double up.
 */
function visibleItems(
  items: readonly MenuItemSpec[],
  registry: CommandRegistry<AppCommandId>,
): MenuItemSpec[] {
  const shown: MenuItemSpec[] = [];
  for (const item of items) {
    if (item.kind === "command" && !registry.get(item.command)) continue;
    if (item.kind === "separator" && (shown.length === 0 || shown.at(-1)?.kind === "separator")) {
      continue;
    }
    shown.push(item);
  }
  if (shown.at(-1)?.kind === "separator") shown.pop();
  return shown;
}

/** The application menus. Re-renders whenever the registry changes. */
export function AppMenubar({ registry, menus, shortcutFor }: AppMenubarProps) {
  useSyncExternalStore(registry.subscribe, registry.version);

  const renderItems = (items: readonly MenuItemSpec[], prefix: string): ReactNode[] =>
    visibleItems(items, registry).map((item, index) => {
      const key = `${prefix}${index}`;
      switch (item.kind) {
        case "separator":
          return <MenubarSeparator key={key} />;
        case "command": {
          // An entry with its own arguments (an "Open Recent" folder) isn't what the keys run.
          const shortcut = item.args === undefined ? shortcutFor?.(item.command) : undefined;
          return (
            <MenubarItem
              key={key}
              disabled={!registry.isEnabled(item.command)}
              onSelect={() => {
                void registry.execute(item.command, item.args);
              }}
              {...(shortcut ? { "aria-keyshortcuts": shortcut.aria } : {})}
            >
              {item.label ?? registry.get(item.command)?.title}
              {shortcut ? <MenubarShortcut>{shortcut.label}</MenubarShortcut> : null}
            </MenubarItem>
          );
        }
        case "submenu":
          return item.items.length === 0 ? (
            <MenubarItem key={key} disabled>
              {item.label}
            </MenubarItem>
          ) : (
            <MenubarSub key={key}>
              <MenubarSubTrigger>{item.label}</MenubarSubTrigger>
              <MenubarSubContent>{renderItems(item.items, `${key}.`)}</MenubarSubContent>
            </MenubarSub>
          );
        default:
          return assertNever(item);
      }
    });

  return (
    <Menubar aria-label="Application" className="app-shell__menubar">
      {menus.map((menu) => {
        const items = renderItems(menu.items, `${menu.label}.`);
        return (
          <MenubarMenu key={menu.label}>
            {/* Kept in its place, so the menus don't move when there's nothing in one. */}
            <MenubarTrigger disabled={items.length === 0}>{menu.label}</MenubarTrigger>
            <MenubarContent>{items}</MenubarContent>
          </MenubarMenu>
        );
      })}
    </Menubar>
  );
}

function assertNever(value: never): never {
  throw new Error(`Unexpected menu item: ${JSON.stringify(value)}`);
}
