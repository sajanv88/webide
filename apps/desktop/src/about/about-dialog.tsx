import {
  Button,
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogTitle,
} from "@nuvui/react";
import type { AppInfoDto } from "@repo/contracts";

interface AboutDialogProps {
  readonly info: AppInfoDto | null;
  readonly open: boolean;
  readonly onOpenChange: (open: boolean) => void;
}

/** Help → About: the name and version from the app's config. */
export function AboutDialog({ info, open, onOpenChange }: AboutDialogProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent size="sm">
        <DialogTitle>About {info?.name ?? "this app"}</DialogTitle>
        <DialogDescription>
          Version {info?.version ?? "unknown"}. A free, open-source IDE for JavaScript and
          TypeScript.
        </DialogDescription>
        <DialogFooter>
          <DialogClose asChild>
            <Button intent="secondary">Close</Button>
          </DialogClose>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
