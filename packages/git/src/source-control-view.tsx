import "./source-control-view.scss";
import { GitBranchIcon, GitCommitIcon, RefreshIcon } from "@nuvui/icons";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogTitle,
  Button,
  Empty,
  EmptyActions,
  EmptyDescription,
  EmptyTitle,
  Field,
  FieldControl,
  FieldDescription,
  FieldError,
  FieldLabel,
  ScrollArea,
  Spinner,
  Textarea,
  toast,
} from "@nuvui/react";
import type { AppCommandId, AppEvents, GitDiffDto, GitFileDto, IpcResult } from "@repo/contracts";
import type { CommandRegistry, EventBus } from "@repo/kernel";
import {
  type FormEvent,
  type KeyboardEvent,
  type ReactNode,
  useEffect,
  useId,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import { ChangeList, type ListKind } from "./change-list";
import { DiffDialog, type DiffRequest, type DiffSlotProps } from "./diff-dialog";
import type { GitStore } from "./git-store";

/** How long changes on disk have to settle before git is asked again. */
export const REFRESH_DELAY_MS = 300;

export interface SourceControlViewProps {
  /** Git for the open workspace, from `createGitStore`. */
  readonly store: GitStore;
  /** Reads a file's two versions, for its diff. */
  readonly readDiff: (path: string, side: "staged" | "unstaged") => Promise<IpcResult<GitDiffDto>>;
  readonly registry: CommandRegistry<AppCommandId>;
  readonly bus: EventBus<AppEvents>;
  /** The shell view this is in; `git.commit` with nothing typed shows it. */
  readonly viewId: string;
  /** Draws two versions of a file side by side (the app's diff surface). */
  readonly renderDiff: (props: DiffSlotProps) => ReactNode;
  /** CSP nonce for nuvui ScrollArea's `<style>` element. */
  readonly styleNonce?: string | undefined;
}

/**
 * Source Control: the branch, a commit message, and the changed files in
 * three lists (conflicts, staged, changes) to stage, unstage, discard, and
 * compare. It keeps itself current as files change on disk.
 */
export function SourceControlView({
  store,
  readDiff,
  registry,
  bus,
  viewId,
  renderDiff,
  styleNonce,
}: SourceControlViewProps) {
  const headingId = useId();
  const state = useSyncExternalStore(store.subscribe, store.getState);
  const { view, busy } = state;
  const [message, setMessage] = useState("");
  const [commitError, setCommitError] = useState<string | undefined>(undefined);
  const [diff, setDiff] = useState<DiffRequest | null>(null);
  const [diffResult, setDiffResult] = useState<IpcResult<GitDiffDto> | null>(null);
  const [discarding, setDiscarding] = useState<GitFileDto | null>(null);
  const messageRef = useRef<HTMLTextAreaElement>(null);
  const sectionRef = useRef<HTMLElement>(null);
  // What had focus when a dialog opened, to give it back when it closes.
  const returnTo = useRef<HTMLElement | null>(null);
  const remember = () => {
    returnTo.current =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
  };
  // Back where it was, or (the row went: staged, discarded) the first changed file.
  const restoreFocus = () => {
    const before = returnTo.current;
    returnTo.current = null;
    if (before?.isConnected) {
      before.focus();
      return;
    }
    const target =
      sectionRef.current?.querySelector<HTMLElement>('[role="row"]') ?? messageRef.current;
    target?.focus();
  };

  // Read git's state now, after changes on disk settle (git's own files in
  // .git included), and when the window comes back into focus.
  useEffect(() => {
    void store.refresh();
    let timer: ReturnType<typeof setTimeout> | undefined;
    const soon = () => {
      clearTimeout(timer);
      timer = setTimeout(() => void store.refresh(), REFRESH_DELAY_MS);
    };
    const stop = bus.on("workspace.filesChanged", soon);
    window.addEventListener("focus", soon);
    return () => {
      clearTimeout(timer);
      stop();
      window.removeEventListener("focus", soon);
    };
  }, [store, bus]);

  const isRepo = view.status === "repository";
  const stagedCount = view.status === "repository" ? view.staged.length : 0;

  const commit = async () => {
    if (message.trim() === "") {
      setCommitError("Write a message for the commit.");
      messageRef.current?.focus();
      return;
    }
    if (stagedCount === 0) {
      setCommitError("Stage the changes to commit first.");
      return;
    }
    setCommitError(undefined);
    const result = await store.commit(message);
    if (result.ok) {
      setMessage("");
      toast("Committed");
    } else {
      setCommitError(result.error.message);
      messageRef.current?.focus();
    }
  };
  const commitRef = useRef(commit);
  commitRef.current = commit;

  // Commands, while there's a repository.
  useEffect(() => {
    const disposers = [
      registry.register({
        id: "git.refresh",
        title: "Refresh Source Control",
        category: "Git",
        run: () => store.refresh(),
      }),
    ];
    if (isRepo) {
      disposers.push(
        registry.register({
          id: "git.commit",
          title: "Commit",
          category: "Git",
          run: async () => {
            await registry.execute("view.showView", viewId);
            await commitRef.current();
          },
        }),
        registry.register({
          id: "git.stageAll",
          title: "Stage All Changes",
          category: "Git",
          isEnabled: () => {
            const now = store.getState().view;
            return now.status === "repository" && now.changes.length > 0;
          },
          run: () => report(store.stageAll(), "Couldn't stage the changes"),
        }),
        registry.register({
          id: "git.unstageAll",
          title: "Unstage All Changes",
          category: "Git",
          isEnabled: () => {
            const now = store.getState().view;
            return now.status === "repository" && now.staged.length > 0;
          },
          run: () => report(store.unstageAll(), "Couldn't unstage the changes"),
        }),
      );
    } else if (view.status === "notARepository") {
      disposers.push(
        registry.register({
          id: "git.init",
          title: "Initialize Repository",
          category: "Git",
          run: () => report(store.init(), "Couldn't make a repository here"),
        }),
      );
    }
    const unsubscribe = store.subscribe(() => registry.notify());
    return () => {
      unsubscribe();
      for (const dispose of disposers) dispose();
    };
  }, [registry, store, isRepo, view.status, viewId]);

  const openDiff = (file: GitFileDto, kind: ListKind) => {
    remember();
    const side = kind === "staged" ? "staged" : "unstaged";
    setDiff({ path: file.path, side });
    setDiffResult(null);
    void readDiff(file.path, side).then(setDiffResult);
  };

  const onMessageKey = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    // Ctrl+Enter (Cmd+Enter on macOS) commits, as in other editors.
    if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      void commit();
    }
  };
  const submit = (event: FormEvent) => {
    event.preventDefault();
    void commit();
  };

  return (
    <section ref={sectionRef} className="source-control" aria-labelledby={headingId}>
      <div className="source-control__header">
        <h2 id={headingId} className="source-control__title">
          Source Control
        </h2>
        <Button
          intent="ghost"
          size="sm"
          className="source-control__icon"
          aria-label="Refresh source control"
          onClick={() => void store.refresh()}
        >
          <RefreshIcon size={16} />
        </Button>
      </div>
      <ScrollArea
        className="source-control__scroll"
        aria-label="Source control"
        {...(styleNonce ? { nonce: styleNonce } : {})}
      >
        {view.status === "loading" ? (
          <div className="source-control__loading">
            <Spinner label="Reading git's state" />
          </div>
        ) : null}
        {view.status === "unavailable" ? (
          <Empty className="source-control__empty">
            <EmptyTitle>Git isn't installed</EmptyTitle>
            <EmptyDescription>
              Source control runs the git program, and none was found. Install git, make sure it's
              on the PATH, then refresh.
            </EmptyDescription>
          </Empty>
        ) : null}
        {view.status === "restricted" ? (
          <Empty className="source-control__empty">
            <EmptyTitle>Source control is off in Restricted Mode</EmptyTitle>
            <EmptyDescription>
              Git can run programs a repository configures, so it doesn't run in a folder whose
              authors you haven't trusted.
            </EmptyDescription>
            <EmptyActions>
              <Button onClick={() => void registry.execute("workspace.manageTrust")}>
                Manage Workspace Trust
              </Button>
            </EmptyActions>
          </Empty>
        ) : null}
        {view.status === "notARepository" ? (
          <Empty className="source-control__empty">
            <EmptyTitle>Not a git repository</EmptyTitle>
            <EmptyDescription>This folder isn't in a git repository.</EmptyDescription>
            <EmptyActions>
              <Button
                disabled={busy}
                onClick={() => void report(store.init(), "Couldn't make a repository here")}
              >
                Initialize repository
              </Button>
            </EmptyActions>
          </Empty>
        ) : null}
        {view.status === "failed" ? (
          <Empty className="source-control__empty">
            <EmptyTitle>Couldn't read git's state</EmptyTitle>
            <EmptyDescription>{view.message}</EmptyDescription>
            <EmptyActions>
              <Button onClick={() => void store.refresh()}>Try again</Button>
            </EmptyActions>
          </Empty>
        ) : null}
        {view.status === "repository" ? (
          <div className="source-control__body">
            <p className="source-control__branch">
              <GitBranchIcon size={14} />
              {view.head.branch ? (
                <span>
                  On <strong>{view.head.branch}</strong>
                </span>
              ) : (
                <span>
                  {view.head.commit ? `Detached at ${view.head.commit}` : "No commits yet"}
                </span>
              )}
              {view.head.ahead > 0 ? <span>, {view.head.ahead} to push</span> : null}
              {view.head.behind > 0 ? <span>, {view.head.behind} to pull</span> : null}
            </p>
            <form className="source-control__commit" onSubmit={submit} aria-label="Commit">
              <Field controlId="git-commit-message">
                <FieldLabel className="source-control__label">Commit message</FieldLabel>
                <FieldControl>
                  <Textarea
                    ref={messageRef}
                    value={message}
                    onChange={(event) => {
                      setMessage(event.currentTarget.value);
                      setCommitError(undefined);
                    }}
                    onKeyDown={onMessageKey}
                    rows={3}
                    autoResize
                    spellCheck
                  />
                </FieldControl>
                <FieldDescription className="source-control__hint">
                  Ctrl+Enter (Cmd+Enter on macOS) commits what's staged.
                </FieldDescription>
                <FieldError>{commitError}</FieldError>
              </Field>
              <Button type="submit" disabled={busy} className="source-control__commit-button">
                <GitCommitIcon size={16} />
                {stagedCount > 0
                  ? `Commit ${stagedCount} ${stagedCount === 1 ? "file" : "files"}`
                  : "Commit"}
              </Button>
            </form>
            {view.conflicts.length + view.staged.length + view.changes.length === 0 ? (
              <p className="source-control__clean" role="status">
                No changes.
              </p>
            ) : null}
            {(
              [
                ["conflicts", "Merge conflicts", view.conflicts],
                ["staged", "Staged changes", view.staged],
                ["changes", "Changes", view.changes],
              ] as const
            ).map(([kind, title, files]) => (
              <ChangeList
                key={kind}
                kind={kind}
                title={title}
                files={files}
                busy={busy}
                onOpen={openDiff}
                onStage={(paths) => void report(store.stage(paths), "Couldn't stage")}
                onUnstage={(paths) => void report(store.unstage(paths), "Couldn't unstage")}
                onDiscard={(file) => {
                  remember();
                  setDiscarding(file);
                }}
              />
            ))}
            {view.truncated ? (
              <p className="source-control__clean">
                More files changed than are listed here. Commit or stage some to see the rest.
              </p>
            ) : null}
          </div>
        ) : null}
      </ScrollArea>
      <DiffDialog
        request={diff}
        result={diffResult}
        onClose={() => setDiff(null)}
        onOpenFile={(path) => {
          setDiff(null);
          bus.emit("file.openRequested", { path });
        }}
        renderDiff={renderDiff}
        onClosed={restoreFocus}
      />
      <AlertDialog
        open={discarding !== null}
        onOpenChange={(open) => {
          if (!open) setDiscarding(null);
        }}
      >
        <AlertDialogContent
          size="sm"
          onCloseAutoFocus={(event) => {
            event.preventDefault();
            restoreFocus();
          }}
        >
          <AlertDialogTitle>Discard changes to {discarding?.path}?</AlertDialogTitle>
          <AlertDialogDescription>
            The file goes back to what's staged, or to the last commit. Its unstaged changes are
            lost, and this can't be undone.
          </AlertDialogDescription>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              intent="danger"
              onClick={() => {
                const file = discarding;
                if (file) void report(store.discard([file.path]), "Couldn't discard the changes");
              }}
            >
              Discard changes
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </section>
  );
}

/** Says when an operation failed, in git's own words. */
async function report(pending: Promise<IpcResult<null>>, title: string): Promise<void> {
  const result = await pending;
  if (!result.ok) toast(title, { intent: "danger", description: result.error.message });
}
