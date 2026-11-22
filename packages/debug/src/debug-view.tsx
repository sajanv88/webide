import "./debug-view.scss";
import {
  ArrowDownIcon,
  ArrowUpIcon,
  CloseIcon,
  PauseIcon,
  PlayIcon,
  RedoIcon,
  SquareIcon,
} from "@nuvui/icons";
import {
  Alert,
  AlertDescription,
  Button,
  Input,
  ListView,
  ListViewItem,
  ScrollArea,
  Toolbar,
  ToolbarButton,
  Tree,
  TreeItem,
  VisuallyHidden,
} from "@nuvui/react";
import type { AppCommandId, VariableDto } from "@repo/contracts";
import type { CommandRegistry } from "@repo/kernel";
import { type ReactNode, useEffect, useId, useState, useSyncExternalStore } from "react";
import { type DebugStore, debuggable } from "./debug-store";

export interface DebugViewProps {
  readonly store: DebugStore;
  readonly registry: CommandRegistry<AppCommandId>;
  /** The file the editor shows, which Run and Debug runs. */
  readonly currentFile: () => string | undefined;
  /** The shell view this is in. */
  readonly viewId: string;
  readonly styleNonce?: string | undefined;
}

const nameOf = (path: string) => path.slice(path.lastIndexOf("/") + 1);

/**
 * Run and Debug: runs the file shown under the debugger; while it runs, a
 * toolbar (continue, pause, step over, into and out, stop), the call stack,
 * the variables, the breakpoints, and a console to evaluate in. It registers
 * the debug commands.
 */
export function DebugView({ store, registry, currentFile, viewId, styleNonce }: DebugViewProps) {
  const headingId = useId();
  const state = useSyncExternalStore(store.subscribe, store.getState);
  const live = state.phase === "running" || state.phase === "paused";

  useEffect(() => {
    const now = () => store.getState();
    const active = () => now().phase === "running" || now().phase === "paused";
    const disposers = [
      registry.register({
        id: "debug.start",
        title: "Start Debugging",
        category: "Debug",
        isEnabled: () => {
          if (now().phase === "paused") return true;
          const file = currentFile();
          return !active() && now().phase !== "starting" && file !== undefined && debuggable(file);
        },
        run: async () => {
          // F5 while paused continues, as in other editors.
          if (now().phase === "paused") return store.resume();
          const file = currentFile();
          if (!file) return;
          await registry.execute("view.showView", viewId);
          await store.start(file);
        },
      }),
      registry.register({
        id: "debug.stop",
        title: "Stop Debugging",
        category: "Debug",
        isEnabled: active,
        run: () => store.stop(),
      }),
      registry.register({
        id: "debug.continue",
        title: "Continue",
        category: "Debug",
        isEnabled: () => now().phase === "paused",
        run: () => store.resume(),
      }),
      registry.register({
        id: "debug.pause",
        title: "Pause",
        category: "Debug",
        isEnabled: () => now().phase === "running",
        run: () => store.pause(),
      }),
      registry.register({
        id: "debug.stepOver",
        title: "Step Over",
        category: "Debug",
        isEnabled: () => now().phase === "paused",
        run: () => store.step("over"),
      }),
      registry.register({
        id: "debug.stepInto",
        title: "Step Into",
        category: "Debug",
        isEnabled: () => now().phase === "paused",
        run: () => store.step("into"),
      }),
      registry.register({
        id: "debug.stepOut",
        title: "Step Out",
        category: "Debug",
        isEnabled: () => now().phase === "paused",
        run: () => store.step("out"),
      }),
    ];
    const unsubscribe = store.subscribe(() => registry.notify());
    return () => {
      unsubscribe();
      for (const dispose of disposers) dispose();
    };
  }, [registry, store, currentFile, viewId]);

  const file = currentFile();
  const run = (command: AppCommandId) => () => void registry.execute(command);

  return (
    <section className="debug-view" aria-labelledby={headingId}>
      <div className="debug-view__header">
        <h2 id={headingId} className="debug-view__title">
          Run and Debug
        </h2>
      </div>
      <ScrollArea
        className="debug-view__scroll"
        aria-label="Run and debug"
        {...(styleNonce ? { nonce: styleNonce } : {})}
      >
        <div className="debug-view__body">
          {live || state.phase === "starting" ? (
            <Toolbar aria-label="Debug" className="debug-view__toolbar">
              {state.phase === "paused" ? (
                <ToolbarButton
                  aria-label="Continue (F5)"
                  title="Continue (F5)"
                  onClick={run("debug.continue")}
                >
                  <PlayIcon size={16} />
                </ToolbarButton>
              ) : (
                <ToolbarButton
                  aria-label="Pause"
                  title="Pause"
                  disabled={state.phase !== "running"}
                  onClick={run("debug.pause")}
                >
                  <PauseIcon size={16} />
                </ToolbarButton>
              )}
              <ToolbarButton
                aria-label="Step over (F10)"
                title="Step over (F10)"
                disabled={state.phase !== "paused"}
                onClick={run("debug.stepOver")}
              >
                <RedoIcon size={16} />
              </ToolbarButton>
              <ToolbarButton
                aria-label="Step into (F11)"
                title="Step into (F11)"
                disabled={state.phase !== "paused"}
                onClick={run("debug.stepInto")}
              >
                <ArrowDownIcon size={16} />
              </ToolbarButton>
              <ToolbarButton
                aria-label="Step out (Shift+F11)"
                title="Step out (Shift+F11)"
                disabled={state.phase !== "paused"}
                onClick={run("debug.stepOut")}
              >
                <ArrowUpIcon size={16} />
              </ToolbarButton>
              <ToolbarButton
                aria-label="Stop (Shift+F5)"
                title="Stop (Shift+F5)"
                disabled={!live}
                onClick={run("debug.stop")}
              >
                <SquareIcon size={16} />
              </ToolbarButton>
            </Toolbar>
          ) : (
            <div className="debug-view__start">
              <Button
                disabled={!file || !debuggable(file)}
                onClick={run("debug.start")}
                className="debug-view__start-button"
              >
                <PlayIcon size={16} />
                {file && debuggable(file) ? `Run and debug ${nameOf(file)}` : "Run and debug"}
              </Button>
              <p className="debug-view__hint">
                {file && debuggable(file)
                  ? "Runs the file shown with Node (F5). F9 or a click beside a line number sets a breakpoint."
                  : "Open a JavaScript or TypeScript file to run it with Node."}
              </p>
            </div>
          )}
          <p className="debug-view__status" role="status">
            {state.phase === "starting"
              ? "Starting…"
              : state.phase === "running"
                ? `Running ${state.program ? nameOf(state.program) : ""}`
                : state.phase === "paused"
                  ? (state.pausedBecause ?? "Paused")
                  : state.phase === "ended"
                    ? "The program ended."
                    : ""}
          </p>
          {state.error ? (
            <Alert intent="danger">
              <AlertDescription>{state.error}</AlertDescription>
            </Alert>
          ) : null}

          {state.phase === "paused" && state.frames.length > 0 ? (
            <Section title="Call stack">
              {(labelId) => (
                <ListView
                  aria-labelledby={labelId}
                  selectionMode="single"
                  value={state.frameId === null ? [] : [String(state.frameId)]}
                  onValueChange={(value) => {
                    const id = Number(value[0]);
                    if (value[0]) void store.selectFrame(id);
                  }}
                  className="debug-view__list"
                >
                  {state.frames.map((frame) => (
                    <ListViewItem key={frame.id} value={String(frame.id)} textValue={frame.name}>
                      <span className="debug-view__frame">{frame.name}</span>
                      <span className="debug-view__where">
                        {frame.source?.kind === "workspace"
                          ? `${nameOf(frame.source.path)}:${frame.line}`
                          : frame.source?.kind === "outside"
                            ? nameOf(frame.source.name)
                            : ""}
                      </span>
                    </ListViewItem>
                  ))}
                </ListView>
              )}
            </Section>
          ) : null}

          {state.phase === "paused" && state.scopes.length > 0 ? (
            <Section title="Variables">
              {(labelId) => <Variables store={store} labelId={labelId} />}
            </Section>
          ) : null}

          <Section title="Breakpoints">
            {(labelId) =>
              state.breakpoints.size === 0 ? (
                <p className="debug-view__hint">
                  None. F9 or a click beside a line number sets one.
                </p>
              ) : (
                <ListView aria-labelledby={labelId} className="debug-view__list">
                  {[...state.breakpoints].flatMap(([path, lines]) =>
                    lines.map((line) => (
                      <ListViewItem key={`${path}:${line}`} value={`${path}:${line}`}>
                        <span className="debug-view__frame">
                          {nameOf(path)}:{line}
                        </span>
                        <Button
                          intent="ghost"
                          size="sm"
                          className="debug-view__icon"
                          aria-label={`Remove the breakpoint at ${path} line ${line}`}
                          onClick={() => store.removeBreakpoint(path, line)}
                        >
                          <CloseIcon size={14} />
                        </Button>
                      </ListViewItem>
                    )),
                  )}
                </ListView>
              )
            }
          </Section>

          {state.output.length > 0 || live ? <Console store={store} live={live} /> : null}
        </div>
      </ScrollArea>
    </section>
  );
}

function Section({
  title,
  children,
}: {
  readonly title: string;
  readonly children: (labelId: string) => ReactNode;
}) {
  const id = useId();
  return (
    <section className="debug-view__section" aria-labelledby={id}>
      <h3 id={id} className="debug-view__section-title">
        {title}
      </h3>
      {children(id)}
    </section>
  );
}

/** The paused frame's scopes as a tree: objects open to their properties. */
function Variables({ store, labelId }: { readonly store: DebugStore; readonly labelId: string }) {
  const state = useSyncExternalStore(store.subscribe, store.getState);
  const [loaded, setLoaded] = useState<ReadonlyMap<number, readonly VariableDto[]>>(new Map());
  const [expanded, setExpanded] = useState<string[]>(() =>
    state.scopes
      .filter((scope) => scope.variables !== null)
      .map((scope) => `s:${scope.scope.reference}`),
  );

  const onExpandedChange = (next: string[]) => {
    setExpanded(next);
    for (const value of next) {
      const reference = Number(value.slice(2));
      const known =
        loaded.has(reference) ||
        state.scopes.some((scope) => scope.scope.reference === reference && scope.variables);
      if (!known && reference > 0) {
        void store
          .children(reference)
          .then((children) => setLoaded((now) => new Map(now).set(reference, children)));
      }
    }
  };

  const renderVariables = (
    variables: readonly VariableDto[] | undefined,
    parent: string,
  ): ReactNode[] =>
    variables === undefined
      ? [
          <TreeItem
            key={`${parent}:loading`}
            value={`${parent}:loading`}
            label="Loading…"
            disabled
          />,
        ]
      : variables.map((variable, index) => {
          const value = `v:${variable.reference}:${parent}:${index}`;
          const open = variable.reference > 0 && expanded.includes(`v:${variable.reference}`);
          return (
            <TreeItem
              key={value}
              value={variable.reference > 0 ? `v:${variable.reference}` : value}
              textValue={variable.name}
              label={
                <span className="debug-variable">
                  <span className="debug-variable__name">{variable.name}</span>
                  <VisuallyHidden>: </VisuallyHidden>
                  <span className="debug-variable__value">{variable.value}</span>
                </span>
              }
            >
              {variable.reference > 0
                ? open
                  ? renderVariables(loaded.get(variable.reference), value)
                  : [
                      <TreeItem
                        key={`${value}:closed`}
                        value={`${value}:closed`}
                        label=""
                        disabled
                      />,
                    ]
                : null}
            </TreeItem>
          );
        });

  return (
    <Tree
      aria-labelledby={labelId}
      className="debug-view__tree"
      expanded={expanded}
      onExpandedChange={onExpandedChange}
    >
      {state.scopes.map(({ scope, variables }) => (
        <TreeItem key={scope.reference} value={`s:${scope.reference}`} label={scope.name}>
          {renderVariables(variables ?? loaded.get(scope.reference), `s:${scope.reference}`)}
        </TreeItem>
      ))}
    </Tree>
  );
}

/** What the program printed, and expressions evaluated where it's paused. */
function Console({ store, live }: { readonly store: DebugStore; readonly live: boolean }) {
  const state = useSyncExternalStore(store.subscribe, store.getState);
  const [expression, setExpression] = useState("");
  const id = useId();
  return (
    <section className="debug-view__section" aria-labelledby={id}>
      <h3 id={id} className="debug-view__section-title">
        Debug console
      </h3>
      <div className="debug-view__console" role="log" aria-labelledby={id}>
        {state.output.map((line) => (
          <div
            key={line.id}
            className={`debug-line debug-line--${line.category === "stderr" ? "error" : line.category === "input" ? "input" : "output"}`}
          >
            {line.text}
          </div>
        ))}
      </div>
      {live ? (
        <form
          className="debug-view__evaluate"
          onSubmit={(event) => {
            event.preventDefault();
            const text = expression.trim();
            if (!text) return;
            setExpression("");
            void store.evaluate(text);
          }}
        >
          <Input
            aria-label="Evaluate an expression"
            placeholder="Evaluate (Enter)"
            value={expression}
            onChange={(event) => setExpression(event.currentTarget.value)}
            autoComplete="off"
            spellCheck={false}
          />
        </form>
      ) : null}
    </section>
  );
}
