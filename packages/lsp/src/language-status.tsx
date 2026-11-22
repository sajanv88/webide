import "./language-status.scss";
import { AlertTriangleIcon, BracesIcon, XCircleIcon } from "@nuvui/icons";
import { Button, Spinner } from "@nuvui/react";
import type { AppCommandId, AppEvents, EditorProblem, LanguageStatusDto } from "@repo/contracts";
import type { CommandRegistry, EventBus } from "@repo/kernel";
import { useEffect, useState } from "react";
import type { LanguageClient } from "./language-client";

export interface LanguageStatusItemProps {
  readonly client: LanguageClient;
  readonly registry: CommandRegistry<AppCommandId>;
  readonly bus: EventBus<AppEvents>;
}

const count = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

/**
 * The language server in the status bar: starting, ready (with how many
 * problems the open files have), or failed (pressing it starts it again).
 * It registers `language.restart`.
 */
export function LanguageStatusItem({ client, registry, bus }: LanguageStatusItemProps) {
  const [status, setStatus] = useState<LanguageStatusDto | null>(null);
  const [problems, setProblems] = useState<ReadonlyMap<string, readonly EditorProblem[]>>(
    new Map(),
  );

  useEffect(() => bus.on("language.status", setStatus), [bus]);
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
        title: "Restart the TypeScript Server",
        category: "Language",
        run: () => client.restart(),
      }),
    [registry, client],
  );

  if (!status) return null;
  if (status.state === "starting") {
    return (
      <span className="language-status" role="status">
        <Spinner size="sm" label="Starting TypeScript" />
        TypeScript
      </span>
    );
  }
  if (status.state === "failed") {
    return (
      <Button
        intent="ghost"
        size="sm"
        className="language-status language-status--failed"
        title={status.message}
        aria-label={`TypeScript isn't running: ${status.message}. Restart it`}
        onClick={() => void registry.execute("language.restart")}
      >
        <XCircleIcon size={14} />
        TypeScript
      </Button>
    );
  }

  let errors = 0;
  let warnings = 0;
  for (const list of problems.values()) {
    for (const problem of list) {
      if (problem.severity === "error") errors += 1;
      else if (problem.severity === "warning") warnings += 1;
    }
  }
  return (
    <span
      className="language-status"
      role="status"
      aria-label={`TypeScript ready. ${count(errors, "error")}, ${count(warnings, "warning")} in open files`}
    >
      <BracesIcon size={14} />
      TypeScript
      <span className="language-status__count" aria-hidden="true">
        <XCircleIcon size={12} />
        {errors}
        <AlertTriangleIcon size={12} />
        {warnings}
      </span>
    </span>
  );
}
