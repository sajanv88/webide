import "./close-dialog.scss";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogTitle,
} from "@nuvui/react";
import type { CloseChoice, PendingClose } from "./documents";

export interface CloseDialogProps {
  readonly pending: PendingClose | null;
  readonly onAnswer: (choice: CloseChoice) => void;
  /** Where focus goes once it closes: the tab it was on may be gone. */
  readonly onClosed: () => void;
}

/** "Save changes to …?" before documents with unsaved changes close. */
export function CloseDialog({ pending, onAnswer, onClosed }: CloseDialogProps) {
  const dirty = pending?.dirty ?? [];
  const [only] = dirty;
  const saving = pending?.saving ?? false;

  return (
    <AlertDialog
      open={pending !== null}
      onOpenChange={(open) => {
        // Esc, or a button that already answered (then this does nothing).
        if (!open) onAnswer("cancel");
      }}
    >
      <AlertDialogContent
        size="sm"
        onCloseAutoFocus={(event) => {
          event.preventDefault();
          onClosed();
        }}
      >
        <AlertDialogTitle>
          {dirty.length === 1 && only
            ? `Save changes to ${only.name}?`
            : `Save changes to ${dirty.length} files?`}
        </AlertDialogTitle>
        <AlertDialogDescription>
          {dirty.length === 1
            ? "Your changes will be lost if you don't save them."
            : "Your changes to these files will be lost if you don't save them:"}
        </AlertDialogDescription>
        {dirty.length > 1 ? (
          <ul className="close-dialog__files">
            {dirty.map((tab) => (
              <li key={tab.id} className="close-dialog__file">
                {tab.path}
              </li>
            ))}
          </ul>
        ) : null}
        <AlertDialogFooter>
          <AlertDialogCancel disabled={saving}>Cancel</AlertDialogCancel>
          <AlertDialogAction intent="danger" disabled={saving} onClick={() => onAnswer("discard")}>
            Don't save
          </AlertDialogAction>
          <AlertDialogAction
            disabled={saving}
            onClick={(event) => {
              // Stays open while the files are written; it closes when they are.
              event.preventDefault();
              onAnswer("save");
            }}
          >
            {saving ? "Saving…" : "Save"}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
