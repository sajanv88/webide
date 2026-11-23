import "./agent-status.scss";
import { BotIcon } from "@nuvui/icons";
import { Button } from "@nuvui/react";
import type { AppCommandId } from "@repo/contracts";
import type { CommandRegistry } from "@repo/kernel";
import { useSyncExternalStore } from "react";
import type { AgentStore } from "./agent-store";

export interface AgentStatusItemProps {
  readonly store: AgentStore;
  readonly registry: CommandRegistry<AppCommandId>;
}

/**
 * In the status bar while agents can use the folder (or should, but the
 * server couldn't start). Pressing it opens AI Agents.
 */
export function AgentStatusItem({ store, registry }: AgentStatusItemProps) {
  const { status, activity } = useSyncExternalStore(store.subscribe, store.getState);
  if (!status?.enabled) return null;
  const running = status.url !== null;
  const label = running
    ? `AI agents can ${status.allowEdits ? "read and edit" : "read"} this folder. ${activity.length} tool calls`
    : `AI agents are on, but the server isn't running: ${status.error ?? "unknown"}`;
  return (
    <Button
      intent="ghost"
      size="sm"
      className={`agent-status${running ? "" : " agent-status--failed"}`}
      aria-label={`${label}. Open AI Agents`}
      title={label}
      onClick={() => void registry.execute("agents.open")}
    >
      <BotIcon size={14} />
      Agents{running ? "" : " (off)"}
    </Button>
  );
}
