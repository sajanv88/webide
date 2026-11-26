import "./terminal-panel.scss";
import { PlusIcon, TerminalIcon, TrashIcon } from "@nuvui/icons";
import {
  Button,
  Empty,
  EmptyActions,
  EmptyDescription,
  EmptyTitle,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
  VisuallyHidden,
} from "@nuvui/react";
import type { AppCommandId, AppEvents, TerminalPort } from "@repo/contracts";
import type { CommandRegistry, EventBus } from "@repo/kernel";
import { useCallback, useEffect, useRef, useState } from "react";
import { type TerminalStatus, TerminalView } from "./terminal-view";

export interface TerminalPanelProps {
  /** Starts shells in the open workspace's folder. */
  readonly port: TerminalPort;
  readonly registry: CommandRegistry<AppCommandId>;
  readonly bus: EventBus<AppEvents>;
  /** Keys the app takes even inside a terminal (its palette, its views). */
  readonly passKey?: (event: KeyboardEvent) => boolean;
  readonly styleNonce?: string | undefined;
}

interface Entry {
  readonly key: number;
  readonly status: TerminalStatus;
  /** The shell's name, once it's running. */
  readonly shell?: string;
  /** Typed at its prompt once it's up, not run. */
  readonly input?: string;
}

/** A tab's name: its number and shell, and whether it ended. */
function title(index: number, entry: Entry): string {
  const base = `${index + 1}: ${entry.shell ?? "shell"}`;
  if (entry.status.kind === "exited") return `${base} (exited)`;
  if (entry.status.kind === "failed") return `${base} (failed)`;
  return base;
}

/**
 * The terminals, as tabs, in the shell's panel. It registers `terminal.new`,
 * `terminal.kill`, `terminal.focus` (show, start one if there's none, and
 * focus it; or hide the panel if a terminal has focus) and
 * `terminal.screenReader`.
 */
export function TerminalPanel({ port, registry, bus, passKey, styleNonce }: TerminalPanelProps) {
  const [entries, setEntries] = useState<readonly Entry[]>([]);
  const [active, setActive] = useState<number | null>(null);
  const [screenReader, setScreenReader] = useState(false);
  const [themeKey, setThemeKey] = useState("");
  const nextKey = useRef(1);
  const focusers = useRef(new Map<number, () => void>());
  const panelRef = useRef<HTMLDivElement>(null);
  const pendingFocus = useRef<number | null>(null);
  const state = useRef({ entries, active });
  state.current = { entries, active };

  const add = useCallback((input?: string) => {
    const key = nextKey.current++;
    setEntries((now) => [
      ...now,
      { key, status: { kind: "starting" }, ...(input ? { input } : {}) },
    ]);
    setActive(key);
    pendingFocus.current = key;
  }, []);

  const kill = useCallback((key: number | null) => {
    if (key === null) return;
    const { entries: now } = state.current;
    const index = now.findIndex((entry) => entry.key === key);
    const rest = now.filter((entry) => entry.key !== key);
    setEntries(rest);
    focusers.current.delete(key);
    const next = rest[Math.min(index, rest.length - 1)];
    setActive(next?.key ?? null);
    if (next) pendingFocus.current = next.key;
  }, []);

  // Focus a terminal once it's drawn.
  useEffect(() => {
    const key = pendingFocus.current;
    if (key === null) return;
    const focus = focusers.current.get(key);
    if (focus) {
      pendingFocus.current = null;
      requestAnimationFrame(focus);
    }
  });

  useEffect(() => {
    const disposers = [
      registry.register({
        id: "terminal.new",
        title: "New Terminal",
        category: "Terminal",
        run: async () => {
          await registry.execute("view.showPanel");
          add();
        },
      }),
      registry.register({
        id: "terminal.kill",
        title: "Kill Terminal",
        category: "Terminal",
        isEnabled: () => state.current.active !== null,
        run: () => kill(state.current.active),
      }),
      registry.register({
        id: "terminal.focus",
        title: "Toggle Terminal",
        category: "Terminal",
        run: async () => {
          // In a terminal already: hide the panel, back to where you were.
          if (panelRef.current?.contains(document.activeElement)) {
            await registry.execute("view.togglePanel");
            return;
          }
          await registry.execute("view.showPanel");
          const { entries: now, active: shown } = state.current;
          if (now.length === 0) add();
          else if (shown !== null) {
            pendingFocus.current = shown;
            requestAnimationFrame(() => focusers.current.get(shown)?.());
          }
        },
      }),
      registry.register({
        id: "terminal.screenReader",
        title: "Toggle Screen Reader Mode in Terminals",
        category: "Terminal",
        run: () => setScreenReader((now) => !now),
      }),
    ];
    return () => {
      for (const dispose of disposers) dispose();
    };
  }, [registry, add, kill]);

  // Enabled states follow the terminals.
  // biome-ignore lint/correctness/useExhaustiveDependencies: runs when the terminals change
  useEffect(() => registry.notify(), [registry, entries, active]);

  // The page's theme changed: the terminals read their colors again.
  // A command to type for the person (an installer's), in a terminal of its own.
  useEffect(
    () =>
      bus.on("terminal.type", ({ text }) => {
        void registry.execute("view.showPanel").then(() => add(text));
      }),
    [bus, registry, add],
  );

  useEffect(() => {
    const again = () => setThemeKey(String(Date.now()));
    const stop = bus.on("settings.changed", () => requestAnimationFrame(again));
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    media.addEventListener("change", again);
    return () => {
      stop();
      media.removeEventListener("change", again);
    };
  }, [bus]);

  const setStatus = useCallback((key: number, status: TerminalStatus) => {
    setEntries((now) =>
      now.map((entry) =>
        entry.key === key
          ? { ...entry, status, ...(status.kind === "running" ? { shell: status.shell } : {}) }
          : entry,
      ),
    );
  }, []);

  if (entries.length === 0 || active === null) {
    return (
      <div ref={panelRef} className="terminal-panel terminal-panel--empty">
        <Empty>
          <EmptyTitle>No terminal open</EmptyTitle>
          <EmptyDescription>A terminal runs your shell in the folder that's open.</EmptyDescription>
          <EmptyActions>
            <Button onClick={() => add()}>
              <TerminalIcon size={16} />
              New terminal
            </Button>
          </EmptyActions>
        </Empty>
      </div>
    );
  }

  return (
    <div ref={panelRef} className="terminal-panel">
      <Tabs
        className="terminal-panel__tabs"
        value={String(active)}
        onValueChange={(value) => setActive(Number(value))}
      >
        <div className="terminal-panel__bar">
          <TabsList className="terminal-panel__list" aria-label="Terminals">
            {entries.map((entry, index) => (
              <TabsTrigger
                key={entry.key}
                value={String(entry.key)}
                className="terminal-panel__tab"
              >
                <TerminalIcon size={14} />
                {title(index, entry)}
              </TabsTrigger>
            ))}
          </TabsList>
          <div className="terminal-panel__actions">
            <Button
              intent="ghost"
              size="sm"
              className="terminal-panel__icon"
              aria-label="New terminal"
              title="New terminal"
              onClick={() => add()}
            >
              <PlusIcon size={16} />
            </Button>
            <Button
              intent="ghost"
              size="sm"
              className="terminal-panel__icon"
              aria-label="Kill terminal"
              title="Kill terminal"
              onClick={() => kill(active)}
            >
              <TrashIcon size={16} />
            </Button>
          </div>
        </div>
        {entries.map((entry, index) => (
          // Every terminal stays mounted (its shell keeps running); only the
          // chosen one is drawn.
          <TabsContent
            key={entry.key}
            value={String(entry.key)}
            forceMount
            className="terminal-panel__view"
            tabIndex={-1}
          >
            <TerminalView
              port={port}
              label={`Terminal ${title(index, entry)}`}
              active={entry.key === active}
              screenReader={screenReader}
              passKey={passKey}
              styleNonce={styleNonce}
              themeKey={themeKey}
              onStatus={(status) => setStatus(entry.key, status)}
              onReady={(focus) => focusers.current.set(entry.key, focus)}
              initialInput={entry.input}
            />
          </TabsContent>
        ))}
      </Tabs>
      <VisuallyHidden role="status">
        {entries.find((entry) => entry.key === active)?.status.kind === "exited"
          ? "The shell in this terminal exited."
          : ""}
      </VisuallyHidden>
    </div>
  );
}
