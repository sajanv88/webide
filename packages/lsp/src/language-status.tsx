import "./language-status.scss";
import { AlertTriangleIcon, BracesIcon, ShieldAlertIcon, XCircleIcon } from "@nuvui/icons";
import { Button, Spinner } from "@nuvui/react";
import type { AppCommandId, AppEvents, EditorProblem, LanguageStatusDto } from "@repo/contracts";
import type { CommandRegistry, EventBus } from "@repo/kernel";
import { useEffect, useRef, useState } from "react";
import type { LanguageClient } from "./language-client";
import { NodeSetupDialog, type Platform } from "./node-setup";

export interface LanguageStatusItemProps {
  readonly client: LanguageClient;
  readonly registry: CommandRegistry<AppCommandId>;
  readonly bus: EventBus<AppEvents>;
  /** The system, for the Node.js prompt's installer; the browser's guess when absent. */
  readonly platform?: Platform;
}

const count = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

const said: Record<LanguageStatusDto["state"], string> = {
  starting: "starting",
  ready: "ready",
  needsNode: "needs Node.js",
  failed: "isn't running",
  restricted: "is off in Restricted Mode",
};

/**
 * The language servers in the status bar: each one's name as it starts and
 * is ready, with how many problems the open files have. One that failed is
 * a button that starts them again. One that needs Node.js is a button to
 * the prompt for it, which also opens by itself the first time. One that's
 * off until the folder is trusted is a button to `workspace.manageTrust`.
 * It registers `language.restart`.
 */
export function LanguageStatusItem({ client, registry, bus, platform }: LanguageStatusItemProps) {
  const [statuses, setStatuses] = useState<ReadonlyMap<string, LanguageStatusDto>>(new Map());
  const [problems, setProblems] = useState<ReadonlyMap<string, readonly EditorProblem[]>>(
    new Map(),
  );
  const [askingForNode, setAskingForNode] = useState(false);
  const askedForNode = useRef(false);

  useEffect(
    () =>
      bus.on("language.status", ({ server, status }) => {
        setStatuses((now) => new Map(now).set(server, status));
        // Asked once by itself; after that, from its button.
        if (status.state === "needsNode" && !askedForNode.current) {
          askedForNode.current = true;
          setAskingForNode(true);
        }
      }),
    [bus],
  );
  useEffect(
    () =>
      bus.on("language.problems", ({ path, problems: found }) =>
        setProblems((now) => {
          const next = new Map(now);
          if (found.length === 0) next.delete(path);
          else next.set(path, found);
          return next;
        }),
      ),
    [bus],
  );
  useEffect(
    () =>
      registry.register({
        id: "language.restart",
        title: "Restart the Language Servers",
        category: "Language",
        run: async () => {
          setStatuses(new Map());
          await client.restart();
        },
      }),
    [registry, client],
  );

  if (statuses.size === 0) return null;
  const servers = [...statuses];
  const needNode = servers
    .filter(([, status]) => status.state === "needsNode")
    .map(([name]) => name);

  let errors = 0;
  let warnings = 0;
  for (const list of problems.values()) {
    for (const problem of list) {
      if (problem.severity === "error") errors += 1;
      else if (problem.severity === "warning") warnings += 1;
    }
  }
  const summary = `Language servers: ${servers
    .map(([name, status]) => `${name} ${said[status.state]}`)
    .join(", ")}. ${count(errors, "error")}, ${count(warnings, "warning")} in open files`;

  return (
    <span className="language-status">
      <span className="language-status__summary" role="status" aria-label={summary}>
        <BracesIcon size={14} aria-hidden />
        {servers
          .filter(([, status]) => status.state === "starting" || status.state === "ready")
          .map(([name, status]) => (
            <span key={name} className="language-status__server" aria-hidden="true">
              {status.state === "starting" ? (
                <Spinner size="sm" label={`Starting ${name}`} />
              ) : null}
              {name}
            </span>
          ))}
        <span className="language-status__count" aria-hidden="true">
          <XCircleIcon size={12} />
          {errors}
          <AlertTriangleIcon size={12} />
          {warnings}
        </span>
      </span>
      {servers.map(([name, status]) =>
        status.state === "failed" ? (
          <Button
            key={name}
            intent="ghost"
            size="sm"
            className="language-status__button language-status__button--failed"
            title={status.message}
            aria-label={`${name} isn't running: ${status.message}. Restart the language servers`}
            onClick={() => void registry.execute("language.restart")}
          >
            <XCircleIcon size={14} />
            {name}
          </Button>
        ) : status.state === "restricted" ? (
          <Button
            key={name}
            intent="ghost"
            size="sm"
            className="language-status__button"
            title={`${name} loads the project's own code: trust the folder to turn it on`}
            aria-label={`${name} is off in Restricted Mode. Manage workspace trust`}
            onClick={() => void registry.execute("workspace.manageTrust")}
          >
            <ShieldAlertIcon size={14} />
            {name}
          </Button>
        ) : null,
      )}
      {needNode.length > 0 ? (
        <Button
          intent="ghost"
          size="sm"
          className="language-status__button language-status__button--node"
          aria-label={`${needNode.join(", ")} need${needNode.length === 1 ? "s" : ""} Node.js. Set it up`}
          onClick={() => setAskingForNode(true)}
        >
          <AlertTriangleIcon size={14} />
          Node.js needed
        </Button>
      ) : null}
      <NodeSetupDialog
        open={askingForNode}
        onOpenChange={setAskingForNode}
        servers={needNode}
        bus={bus}
        onCheckAgain={() => void registry.execute("language.restart")}
        {...(platform ? { platform } : {})}
      />
    </span>
  );
}
