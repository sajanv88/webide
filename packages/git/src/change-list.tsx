import "./change-list.scss";
import { MinusIcon, PlusIcon, UndoIcon } from "@nuvui/icons";
import { Button, ListView, ListViewItem, VisuallyHidden } from "@nuvui/react";
import type { GitChangeDto, GitFileDto } from "@repo/contracts";
import { FileTypeIcon } from "@repo/file-icons";
import { useId } from "react";

/** Which list a file is in: it decides its change, and its buttons. */
export type ListKind = "staged" | "changes" | "conflicts";

const WORDS: Record<GitChangeDto, string> = {
  modified: "modified",
  added: "added",
  deleted: "deleted",
  renamed: "renamed",
  copied: "copied",
  typeChanged: "type changed",
  untracked: "untracked",
  conflicted: "conflict",
};

const LETTERS: Record<GitChangeDto, string> = {
  modified: "M",
  added: "A",
  deleted: "D",
  renamed: "R",
  copied: "C",
  typeChanged: "T",
  untracked: "U",
  conflicted: "!",
};

/** The BEM modifier for a change (kebab-case). */
const modifier = (change: GitChangeDto): string =>
  change === "typeChanged" ? "type-changed" : change;

/** The change a file shows in a list. */
export function changeIn(file: GitFileDto, kind: ListKind): GitChangeDto {
  if (kind === "staged") return file.staged ?? "modified";
  return file.unstaged ?? "modified";
}

const nameOf = (path: string) => path.slice(path.lastIndexOf("/") + 1);
const folderOf = (path: string) => path.slice(0, Math.max(0, path.lastIndexOf("/")));

export interface ChangeListProps {
  readonly kind: ListKind;
  readonly title: string;
  readonly files: readonly GitFileDto[];
  readonly busy: boolean;
  /** Enter on a row, or a press: show what changed. */
  readonly onOpen: (file: GitFileDto, kind: ListKind) => void;
  readonly onStage: (paths: readonly string[]) => void;
  readonly onUnstage: (paths: readonly string[]) => void;
  readonly onDiscard: (file: GitFileDto) => void;
}

/** One list of changed files (staged, changes, conflicts) as a nuvui ListView, with its buttons. */
export function ChangeList({
  kind,
  title,
  files,
  busy,
  onOpen,
  onStage,
  onUnstage,
  onDiscard,
}: ChangeListProps) {
  const headingId = useId();
  if (files.length === 0) return null;
  const byPath = new Map(files.map((file) => [file.path, file]));
  const all = files.map((file) => file.path);
  return (
    <section className="change-list" aria-labelledby={headingId}>
      <div className="change-list__header">
        <h3 id={headingId} className="change-list__title">
          {title} <span className="change-list__count">({files.length})</span>
        </h3>
        {kind === "staged" ? (
          <Button
            intent="ghost"
            size="sm"
            className="change-list__icon"
            aria-label="Unstage all changes"
            title="Unstage all changes"
            disabled={busy}
            onClick={() => onUnstage(all)}
          >
            <MinusIcon size={16} />
          </Button>
        ) : (
          <Button
            intent="ghost"
            size="sm"
            className="change-list__icon"
            aria-label={kind === "conflicts" ? "Stage all resolved files" : "Stage all changes"}
            title={kind === "conflicts" ? "Stage all resolved files" : "Stage all changes"}
            disabled={busy}
            onClick={() => onStage(all)}
          >
            <PlusIcon size={16} />
          </Button>
        )}
      </div>
      <ListView
        aria-labelledby={headingId}
        className="change-list__list"
        onAction={(path) => {
          const file = byPath.get(path);
          if (file) onOpen(file, kind);
        }}
      >
        {files.map((file) => {
          const change = changeIn(file, kind);
          const name = nameOf(file.path);
          const folder = folderOf(file.path);
          return (
            <ListViewItem key={file.path} value={file.path} textValue={name}>
              <FileTypeIcon name={name} className="change-list__file-icon" />
              <span className="change-list__name">
                {name}
                {folder ? <span className="change-list__folder">{folder}</span> : null}
                {file.from && kind === "staged" ? (
                  <span className="change-list__folder">from {file.from}</span>
                ) : null}
              </span>
              {kind === "changes" && change !== "untracked" ? (
                <Button
                  intent="ghost"
                  size="sm"
                  className="change-list__icon"
                  aria-label={`Discard changes to ${file.path}`}
                  title="Discard changes"
                  disabled={busy}
                  onClick={() => onDiscard(file)}
                >
                  <UndoIcon size={16} />
                </Button>
              ) : null}
              {kind === "staged" ? (
                <Button
                  intent="ghost"
                  size="sm"
                  className="change-list__icon"
                  aria-label={`Unstage ${file.path}`}
                  title="Unstage"
                  disabled={busy}
                  onClick={() => onUnstage([file.path])}
                >
                  <MinusIcon size={16} />
                </Button>
              ) : (
                <Button
                  intent="ghost"
                  size="sm"
                  className="change-list__icon"
                  aria-label={
                    kind === "conflicts" ? `Mark ${file.path} resolved` : `Stage ${file.path}`
                  }
                  title={kind === "conflicts" ? "Mark resolved (stage)" : "Stage"}
                  disabled={busy}
                  onClick={() => onStage([file.path])}
                >
                  <PlusIcon size={16} />
                </Button>
              )}
              <span
                className={`change-list__change change-list__change--${modifier(change)}`}
                title={WORDS[change]}
              >
                <span aria-hidden="true">{LETTERS[change]}</span>
                <VisuallyHidden>, {WORDS[change]}</VisuallyHidden>
              </span>
            </ListViewItem>
          );
        })}
      </ListView>
    </section>
  );
}
