import "./workspace-trust.scss";
import {
  Alert,
  AlertDescription,
  Button,
  Checkbox,
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogTitle,
} from "@nuvui/react";
import type { AppCommandId, TrustPort, WorkspaceDto } from "@repo/contracts";
import type { CommandRegistry } from "@repo/kernel";
import { useEffect, useId, useRef, useState } from "react";

export interface WorkspaceTrustProps {
  /** The open folder, and whether it's trusted now. */
  readonly workspace: WorkspaceDto;
  readonly trusted: boolean;
  readonly port: TrustPort;
  readonly registry: CommandRegistry<AppCommandId>;
  /** The folder's trust changed (the backend has started or stopped what needs it). */
  readonly onChange: (trusted: boolean) => void;
}

/** What stays off in Restricted Mode, and why, as the dialog lists it. */
export const RESTRICTED: readonly string[] = [
  "Source control: git can run programs a repository configures.",
  "Run and Debug, which runs the folder's code.",
  "Language servers that load the project's own code or configuration (ESLint, Svelte).",
];

/** The folder a path is in, as people write it; `null` at the top of a drive. */
export function parentOf(path: string): string | null {
  const trimmed = path.replace(/[\\/]+$/, "");
  const cut = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
  if (cut <= 0) return null;
  const parent = trimmed.slice(0, cut);
  // `C:` alone is the drive: keep its separator.
  return /^[A-Za-z]:$/.test(parent) ? `${parent}\\` : parent;
}

/**
 * Asks whether the person trusts the authors of the open folder's files, the
 * first time an untrusted folder opens, and whenever `workspace.manageTrust`
 * runs. Until they do, the folder is in Restricted Mode: the backend runs
 * nothing from it (see `RESTRICTED`). Closing the dialog leaves it so.
 */
export function WorkspaceTrust({
  workspace,
  trusted,
  port,
  registry,
  onChange,
}: WorkspaceTrustProps) {
  const [open, setOpen] = useState(false);
  const [parent, setParent] = useState(false);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const parentId = useId();
  const parentPath = parentOf(workspace.path);

  // Asked once per folder opened, when it isn't trusted.
  const asked = useRef<string | null>(null);
  useEffect(() => {
    if (asked.current === workspace.id) return;
    asked.current = workspace.id;
    if (!workspace.trusted) setOpen(true);
  }, [workspace]);

  useEffect(
    () =>
      registry.register({
        id: "workspace.manageTrust",
        title: "Manage Workspace Trust…",
        category: "Workspaces",
        run: () => setOpen(true),
      }),
    [registry],
  );

  useEffect(() => {
    if (!open) {
      setFailure(null);
      setParent(false);
    }
  }, [open]);

  const decide = async (next: boolean) => {
    setBusy(true);
    setFailure(null);
    const result = await port.setTrust(next, next && parent);
    setBusy(false);
    if (!result.ok) {
      setFailure(result.error.message);
      return;
    }
    onChange(result.value.trusted);
    setOpen(false);
  };

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent size="md">
        {trusted ? (
          <>
            <DialogTitle>You trust this folder's authors</DialogTitle>
            <DialogDescription>
              Everything works in <span className="workspace-trust__path">{workspace.path}</span>,
              including what runs its code.
            </DialogDescription>
          </>
        ) : (
          <>
            <DialogTitle>Do you trust the authors of the files in this folder?</DialogTitle>
            <DialogDescription>
              <span className="workspace-trust__path">{workspace.path}</span> opens in Restricted
              Mode until you do. Editing, search, the terminal and the other language features work
              either way. In Restricted Mode, these stay off:
            </DialogDescription>
            <ul className="workspace-trust__list">
              {RESTRICTED.map((line) => (
                <li key={line}>{line}</li>
              ))}
            </ul>
            {parentPath ? (
              <div className="workspace-trust__parent">
                <Checkbox
                  id={parentId}
                  checked={parent}
                  onCheckedChange={(checked) => setParent(checked === true)}
                />
                <label htmlFor={parentId}>
                  Trust every folder in <span className="workspace-trust__path">{parentPath}</span>
                </label>
              </div>
            ) : null}
          </>
        )}
        {failure ? (
          <Alert intent="danger">
            <AlertDescription>{failure}</AlertDescription>
          </Alert>
        ) : null}
        <DialogFooter>
          {trusted ? (
            <>
              <Button intent="secondary" disabled={busy} onClick={() => void decide(false)}>
                Don't Trust
              </Button>
              <DialogClose asChild>
                <Button>Close</Button>
              </DialogClose>
            </>
          ) : (
            <>
              <DialogClose asChild>
                <Button intent="secondary">Stay in Restricted Mode</Button>
              </DialogClose>
              <Button disabled={busy} onClick={() => void decide(true)}>
                Trust Folder
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
