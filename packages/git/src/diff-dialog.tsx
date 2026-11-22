import "./diff-dialog.scss";
import { ExternalLinkIcon } from "@nuvui/icons";
import {
  Alert,
  AlertDescription,
  AlertTitle,
  Button,
  Dialog,
  DialogBody,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogTitle,
  Spinner,
} from "@nuvui/react";
import type { GitDiffDto, IpcResult } from "@repo/contracts";
import type { ReactNode } from "react";

/** What the app's diff view is given. */
export interface DiffSlotProps {
  readonly path: string;
  readonly original: string;
  readonly modified: string;
  /** What changed, and in which file. */
  readonly label: string;
}

/** The file whose changes are shown, and which side. */
export interface DiffRequest {
  readonly path: string;
  readonly side: "staged" | "unstaged";
}

export interface DiffDialogProps {
  readonly request: DiffRequest | null;
  readonly result: IpcResult<GitDiffDto> | null;
  readonly onClose: () => void;
  /** Opens the file in the editor. */
  readonly onOpenFile: (path: string) => void;
  readonly renderDiff: (props: DiffSlotProps) => ReactNode;
  /** Where focus goes once it closes (it opened from a row, which may be gone). */
  readonly onClosed: () => void;
}

/** A file's changes in a large dialog: the app's diff view, or why it can't be shown. */
export function DiffDialog({
  request,
  result,
  onClose,
  onOpenFile,
  renderDiff,
  onClosed,
}: DiffDialogProps) {
  const which = request?.side === "staged" ? "Staged changes" : "Changes";
  const title = request ? `${which} to ${request.path}` : "";
  return (
    <Dialog
      open={request !== null}
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <DialogContent
        size="lg"
        className="git-diff"
        onCloseAutoFocus={(event) => {
          event.preventDefault();
          onClosed();
        }}
      >
        <DialogTitle>{title}</DialogTitle>
        <DialogDescription>
          {request?.side === "staged"
            ? "The last commit on the left, what's staged on the right."
            : "What's staged (or the last commit) on the left, the file now on the right."}
        </DialogDescription>
        <DialogBody className="git-diff__body">
          {request && result === null ? <Spinner label="Reading the two versions" /> : null}
          {request && result && !result.ok ? (
            <Alert intent="danger">
              <AlertTitle>Couldn't read the changes</AlertTitle>
              <AlertDescription>{result.error.message}</AlertDescription>
            </Alert>
          ) : null}
          {request && result?.ok && result.value.kind === "binary" ? (
            <Alert>
              <AlertTitle>Binary file</AlertTitle>
              <AlertDescription>
                This file isn't text, so its changes can't be shown.
              </AlertDescription>
            </Alert>
          ) : null}
          {request && result?.ok && result.value.kind === "tooLarge" ? (
            <Alert>
              <AlertTitle>Too large to compare</AlertTitle>
              <AlertDescription>
                A version is over {Math.round(result.value.limit / 1024 / 1024)} MB, the "don't open
                above" size in Settings.
              </AlertDescription>
            </Alert>
          ) : null}
          {request && result?.ok && result.value.kind === "text"
            ? renderDiff({
                path: request.path,
                original: result.value.original,
                modified: result.value.modified,
                label: title,
              })
            : null}
        </DialogBody>
        <DialogFooter>
          {request ? (
            <Button intent="secondary" onClick={() => onOpenFile(request.path)}>
              <ExternalLinkIcon size={16} />
              Open file
            </Button>
          ) : null}
          <DialogClose asChild>
            <Button>Close</Button>
          </DialogClose>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
