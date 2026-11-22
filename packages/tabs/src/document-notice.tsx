import "./document-notice.scss";
import { FileXIcon, LockIcon } from "@nuvui/icons";
import { Alert, AlertActions, AlertDescription, AlertTitle, Button, Spinner } from "@nuvui/react";
import type { ReactNode } from "react";
import type { DocumentView } from "./documents";

const size = (bytes: number): string => {
  const units = ["byte", "kilobyte", "megabyte", "gigabyte"] as const;
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return new Intl.NumberFormat(undefined, {
    style: "unit",
    unit: units[unit],
    unitDisplay: unit === 0 ? "long" : "short",
    maximumFractionDigits: unit === 0 ? 0 : 1,
  }).format(value);
};

export interface DocumentNoticeProps {
  readonly tab: DocumentView;
  /** "Reload": take the file's text, dropping unsaved changes. */
  readonly onReload: () => void;
  /** "Keep my changes": the next save overwrites the file. */
  readonly onKeepMine: () => void;
}

/**
 * What the active tab says besides its text: that it's loading, that it can't
 * be shown, that its file changed or went away, or why it can't be edited.
 */
export function DocumentNotice({ tab, onReload, onKeepMine }: DocumentNoticeProps) {
  const { content } = tab;
  if (content.kind === "loading") {
    return (
      <div className="document-notice">
        <Spinner label={`Opening ${tab.name}`} />
      </div>
    );
  }

  const bars: ReactNode[] = [];
  if (tab.disk === "changed") {
    bars.push(
      <Alert key="changed" intent="warning" className="document-notice__bar">
        <AlertTitle>{tab.name} changed on disk</AlertTitle>
        <AlertDescription>
          It was changed outside the app while you had unsaved changes here.
        </AlertDescription>
        <AlertActions>
          <Button size="sm" onClick={onReload}>
            Reload from disk
          </Button>
          <Button size="sm" intent="ghost" onClick={onKeepMine}>
            Keep my changes
          </Button>
        </AlertActions>
      </Alert>,
    );
  } else if (tab.disk === "deleted") {
    bars.push(
      <Alert key="deleted" intent="warning" className="document-notice__bar">
        <AlertTitle>{tab.name} was deleted</AlertTitle>
        <AlertDescription>
          {content.kind === "text"
            ? "It's no longer on disk. Saving creates it again."
            : "It's no longer on disk."}
        </AlertDescription>
      </Alert>,
    );
  }

  if (content.kind !== "text") {
    const [heading, text] =
      content.kind === "tooLarge"
        ? [
            `${tab.name} is too large to open`,
            `It's ${size(content.size)}. Files over ${size(content.limit)} aren't opened.`,
          ]
        : content.kind === "binary"
          ? [`${tab.name} isn't text`, "It looks like a binary file, so it isn't shown."]
          : [
              `${tab.name} isn't UTF-8 text`,
              "Only files in UTF-8 can be opened. Its text isn't shown, so it can't be changed by mistake.",
            ];
    return (
      <>
        {bars.length > 0 ? <div className="document-notice__bars">{bars}</div> : null}
        <div className="document-notice">
          <div className="document-notice__card">
            <span className="document-notice__icon">
              <FileXIcon size={24} />
            </span>
            <h2 className="document-notice__heading">{heading}</h2>
            <p className="document-notice__text">{text}</p>
          </div>
        </div>
      </>
    );
  }

  if (content.readOnly) {
    bars.push(
      <Alert key="readOnly" className="document-notice__bar" icon={<LockIcon size={16} />}>
        <AlertTitle>Read-only</AlertTitle>
        <AlertDescription>
          {content.readOnly === "tooLargeToEdit"
            ? `${tab.name} is too large to edit here, so it opened read-only.`
            : content.readOnly === "gitInternal"
              ? `${tab.name} is one of git's own files (inside .git), so it can't be changed here.`
              : `${tab.name} is read-only on disk, so it can't be changed here.`}
        </AlertDescription>
      </Alert>,
    );
  } else if (content.lineEnding === "mixed") {
    bars.push(
      <Alert key="mixed" className="document-notice__bar" role="note">
        <AlertTitle>Mixed line endings</AlertTitle>
        <AlertDescription>
          This file has both LF and CRLF line endings. Saving writes LF throughout.
        </AlertDescription>
      </Alert>,
    );
  }
  return bars.length > 0 ? <div className="document-notice__bars">{bars}</div> : null;
}
