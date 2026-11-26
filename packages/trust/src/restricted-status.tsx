import "./restricted-status.scss";
import { ShieldAlertIcon } from "@nuvui/icons";
import { Button } from "@nuvui/react";
import type { AppCommandId } from "@repo/contracts";
import type { CommandRegistry } from "@repo/kernel";

export interface RestrictedStatusItemProps {
  readonly trusted: boolean;
  readonly registry: CommandRegistry<AppCommandId>;
}

/** In the status bar while the folder is in Restricted Mode; pressing it asks again. */
export function RestrictedStatusItem({ trusted, registry }: RestrictedStatusItemProps) {
  if (trusted) return null;
  return (
    <Button
      intent="ghost"
      size="sm"
      className="restricted-status"
      title="This folder isn't trusted: what would run its code stays off"
      onClick={() => void registry.execute("workspace.manageTrust")}
    >
      <ShieldAlertIcon size={14} />
      Restricted Mode
    </Button>
  );
}
