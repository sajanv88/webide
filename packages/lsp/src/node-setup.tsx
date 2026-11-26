import "./node-setup.scss";
import {
  Button,
  Dialog,
  DialogBody,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@nuvui/react";
import type { AppEvents } from "@repo/contracts";
import type { EventBus } from "@repo/kernel";

export type Platform = "windows" | "macos" | "linux";

/** The system the app runs on, from the browser's own words for it. */
export function currentPlatform(userAgent: string = navigator.userAgent): Platform {
  if (/Windows/i.test(userAgent)) return "windows";
  if (/Mac/i.test(userAgent)) return "macos";
  return "linux";
}

/** The command that installs Node.js with the system's own installer; none on Linux, where they differ. */
export const INSTALL_COMMAND: Readonly<Record<Platform, string | null>> = {
  windows: "winget install --id OpenJS.NodeJS.LTS -e",
  macos: "brew install node",
  linux: null,
};

export const DOWNLOAD_ADDRESS = "nodejs.org/en/download";

/** "JSON", "JSON and YAML", "JSON, YAML and CSS". */
export function listed(names: readonly string[]): string {
  if (names.length <= 1) return names[0] ?? "";
  return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

export interface NodeSetupDialogProps {
  readonly open: boolean;
  readonly onOpenChange: (open: boolean) => void;
  /** The servers that need it: "JSON", "YAML"… */
  readonly servers: readonly string[];
  readonly bus: EventBus<AppEvents>;
  /** Looks for Node.js again (the servers start again). */
  readonly onCheckAgain: () => void;
  readonly platform?: Platform;
}

/**
 * Asks for Node.js, which the JSON, YAML, HTML and CSS servers run on: with
 * the system's installer, typed into a terminal for the person to run (never
 * run for them), or by hand from Node's site. Then they look again.
 */
export function NodeSetupDialog({
  open,
  onOpenChange,
  servers,
  bus,
  onCheckAgain,
  platform = currentPlatform(),
}: NodeSetupDialogProps) {
  const command = INSTALL_COMMAND[platform];
  const features = servers.length > 0 ? listed(servers) : "Some language";
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="node-setup">
        <DialogHeader>
          <DialogTitle>Node.js is needed</DialogTitle>
          <DialogDescription>
            {features} features run on Node.js, which isn't installed on this computer, or wasn't
            found. TypeScript and JavaScript work without it.
          </DialogDescription>
        </DialogHeader>
        <DialogBody className="node-setup__body">
          {command ? (
            <section className="node-setup__choice" aria-labelledby="node-setup-install">
              <h3 id="node-setup-install" className="node-setup__heading">
                Install it with {platform === "windows" ? "winget" : "Homebrew"}
              </h3>
              <p className="node-setup__text">
                A terminal opens with this command typed in. Read it, then press Enter to run it.
                {platform === "macos" ? " It needs Homebrew (brew.sh)." : ""}
              </p>
              <code className="node-setup__command">{command}</code>
              <Button
                intent="secondary"
                onClick={() => {
                  bus.emit("terminal.type", { text: command });
                  onOpenChange(false);
                }}
              >
                Open the terminal
              </Button>
            </section>
          ) : null}
          <section className="node-setup__choice" aria-labelledby="node-setup-yourself">
            <h3 id="node-setup-yourself" className="node-setup__heading">
              {command ? "Or install it yourself" : "Install it"}
            </h3>
            <p className="node-setup__text">
              Get the LTS version from{" "}
              <span className="node-setup__address">{DOWNLOAD_ADDRESS}</span>
              {platform === "linux" ? ", or your system's package manager," : ""} and install it.
            </p>
          </section>
          <p className="node-setup__text">Once it's installed, press Check again.</p>
        </DialogBody>
        <DialogFooter>
          <DialogClose asChild>
            <Button intent="ghost">Not now</Button>
          </DialogClose>
          <Button
            onClick={() => {
              onCheckAgain();
              onOpenChange(false);
            }}
          >
            Check again
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
