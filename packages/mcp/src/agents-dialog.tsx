import "./agents-dialog.scss";
import { CheckIcon, CopyIcon, EyeIcon, EyeOffIcon } from "@nuvui/icons";
import {
  Alert,
  AlertDescription,
  AlertTitle,
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
  Field,
  FieldControl,
  FieldDescription,
  FieldLabel,
  Input,
  NumberField,
  Spinner,
  Switch,
  Textarea,
} from "@nuvui/react";
import type { AgentStatusDto, AppCommandId } from "@repo/contracts";
import type { CommandRegistry } from "@repo/kernel";
import { useEffect, useId, useState, useSyncExternalStore } from "react";
import { type AgentStore, claudeCommand, clientConfig } from "./agent-store";

export interface AgentsDialogProps {
  readonly store: AgentStore;
  readonly registry: CommandRegistry<AppCommandId>;
  /** Puts text on the clipboard; the browser's by default. */
  readonly copy?: ((text: string) => Promise<void>) | undefined;
}

const MIN_PORT = 1024;
const MAX_PORT = 65535;

const browserCopy = (text: string) => navigator.clipboard.writeText(text);

/**
 * AI Agents: turns the MCP server on and off, allows edits, picks its port,
 * and shows how to connect an agent (the address, the token, Claude Code's
 * command) and what agents have done. It registers `agents.open`.
 */
export function AgentsDialog({ store, registry, copy = browserCopy }: AgentsDialogProps) {
  const [open, setOpen] = useState(false);
  const state = useSyncExternalStore(store.subscribe, store.getState);

  useEffect(
    () =>
      registry.register({
        id: "agents.open",
        title: "AI Agents…",
        category: "Preferences",
        run: () => setOpen(true),
      }),
    [registry],
  );
  // Read afresh each time it opens.
  useEffect(() => {
    if (open) void store.refresh();
  }, [open, store]);

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent size="lg">
        <DialogTitle>AI Agents</DialogTitle>
        <DialogDescription>
          Let AI agents, such as Claude Code, work in the open folder through an MCP server on this
          computer. Only programs given the token can connect.
        </DialogDescription>
        {state.failure ? (
          <Alert intent="danger">
            <AlertTitle>That didn't work</AlertTitle>
            <AlertDescription>{state.failure}</AlertDescription>
          </Alert>
        ) : null}
        {state.status ? (
          <Settings store={store} status={state.status} busy={state.busy} copy={copy} />
        ) : state.failure ? null : (
          <div className="agents-dialog__loading">
            <Spinner label="Reading the agent settings" />
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}

interface SettingsProps {
  readonly store: AgentStore;
  readonly status: AgentStatusDto;
  readonly busy: boolean;
  readonly copy: (text: string) => Promise<void>;
}

function Settings({ store, status, busy, copy }: SettingsProps) {
  const enabledId = useId();
  const editsId = useId();
  const [port, setPort] = useState<number | null>(status.port);
  useEffect(() => setPort(status.port), [status.port]);
  const portValid = port !== null && Number.isInteger(port) && port >= MIN_PORT && port <= MAX_PORT;
  const configure = (next: Partial<{ enabled: boolean; allowEdits: boolean; port: number }>) =>
    void store.configure({
      enabled: status.enabled,
      allowEdits: status.allowEdits,
      port: status.port,
      ...next,
    });

  return (
    <div className="agents-dialog">
      <div className="agents-dialog__switch">
        <Switch
          id={enabledId}
          checked={status.enabled}
          disabled={busy}
          onCheckedChange={(enabled) => configure({ enabled })}
        />
        <label htmlFor={enabledId}>Let agents use the open folder</label>
      </div>
      <div className="agents-dialog__switch">
        <Switch
          id={editsId}
          checked={status.allowEdits}
          disabled={busy}
          onCheckedChange={(allowEdits) => configure({ allowEdits })}
        />
        <div>
          <label htmlFor={editsId}>Allow agents to edit files</label>
          <p className="agents-dialog__hint">
            Otherwise they only read: files, search, and git's changes. Agents never run commands,
            or change anything in .git.
          </p>
        </div>
      </div>

      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (portValid && port !== status.port) configure({ port });
        }}
      >
        <Field invalid={!portValid}>
          <FieldLabel>Port</FieldLabel>
          <div className="agents-dialog__row">
            <FieldControl>
              <NumberField
                className="agents-dialog__port"
                value={port}
                onValueChange={setPort}
                min={MIN_PORT}
                max={MAX_PORT}
                step={1}
                format={{ options: { useGrouping: false } }}
              />
            </FieldControl>
            <Button
              type="submit"
              intent="secondary"
              disabled={busy || !portValid || port === status.port}
            >
              Use this port
            </Button>
          </div>
          <FieldDescription>
            On 127.0.0.1 only. From {MIN_PORT} to {MAX_PORT}.
          </FieldDescription>
        </Field>
      </form>

      <p className="agents-dialog__status" role="status">
        {busy
          ? "Applying…"
          : status.url
            ? `Running at ${status.url}${status.allowEdits ? ", edits allowed" : ", read only"}.`
            : status.enabled
              ? "Not running."
              : "Off."}
      </p>
      {status.enabled && status.error ? (
        <Alert intent="warning">
          <AlertTitle>The server isn't running</AlertTitle>
          <AlertDescription>{status.error}</AlertDescription>
        </Alert>
      ) : null}

      {status.url ? <Connect store={store} status={status} url={status.url} copy={copy} /> : null}
      <Activity store={store} />
    </div>
  );
}

interface ConnectProps {
  readonly store: AgentStore;
  readonly status: AgentStatusDto;
  readonly url: string;
  readonly copy: (text: string) => Promise<void>;
}

function Connect({ store, status, url, copy }: ConnectProps) {
  const [shown, setShown] = useState(false);
  return (
    <section className="agents-dialog__section" aria-label="Connecting an agent">
      <Field>
        <FieldLabel>Claude Code</FieldLabel>
        <div className="agents-dialog__row">
          <FieldControl>
            <Input
              readOnly
              value={shown ? claudeCommand(url, status.token) : claudeCommand(url, "•".repeat(12))}
              className="agents-dialog__code"
            />
          </FieldControl>
          <CopyButton
            label="Copy the command"
            text={claudeCommand(url, status.token)}
            copy={copy}
          />
        </div>
        <FieldDescription>Run it in a terminal, once.</FieldDescription>
      </Field>
      <Field>
        <FieldLabel>Token</FieldLabel>
        <div className="agents-dialog__row">
          <FieldControl>
            <Input
              readOnly
              // Masked as text, not a password field: password managers keep out.
              value={shown ? status.token : "•".repeat(status.token.length)}
              className="agents-dialog__code"
            />
          </FieldControl>
          <Button
            intent="ghost"
            size="sm"
            aria-label={shown ? "Hide the token" : "Show the token"}
            aria-pressed={shown}
            onClick={() => setShown((now) => !now)}
          >
            {shown ? <EyeOffIcon size={16} /> : <EyeIcon size={16} />}
          </Button>
          <CopyButton label="Copy the token" text={status.token} copy={copy} />
        </div>
        <FieldDescription>
          Agents send it as Authorization: Bearer. Anyone with it can use the open folder as allowed
          here.
        </FieldDescription>
      </Field>
      <Field>
        <FieldLabel>Other clients (mcpServers)</FieldLabel>
        <div className="agents-dialog__row">
          <FieldControl>
            <Textarea
              readOnly
              rows={4}
              value={clientConfig(url, shown ? status.token : "•".repeat(12))}
              className="agents-dialog__code"
            />
          </FieldControl>
          <CopyButton
            label="Copy the configuration"
            text={clientConfig(url, status.token)}
            copy={copy}
          />
        </div>
      </Field>
      <div>
        <Button intent="secondary" onClick={() => void store.newToken()}>
          New token
        </Button>
        <p className="agents-dialog__hint">
          Agents set up with the current token are refused from then on.
        </p>
      </div>
    </section>
  );
}

function CopyButton({
  label,
  text,
  copy,
}: {
  readonly label: string;
  readonly text: string;
  readonly copy: (text: string) => Promise<void>;
}) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 1500);
    return () => clearTimeout(timer);
  }, [copied]);
  return (
    <Button
      intent="ghost"
      size="sm"
      aria-label={copied ? `${label}: copied` : label}
      title={label}
      onClick={() => void copy(text).then(() => setCopied(true))}
    >
      {copied ? <CheckIcon size={16} /> : <CopyIcon size={16} />}
    </Button>
  );
}

/** What agents have done, newest last. */
function Activity({ store }: { readonly store: AgentStore }) {
  const { activity } = useSyncExternalStore(store.subscribe, store.getState);
  const id = useId();
  return (
    <section className="agents-dialog__section" aria-labelledby={id}>
      <h3 id={id} className="agents-dialog__heading">
        What agents did
      </h3>
      {activity.length === 0 ? (
        <p className="agents-dialog__hint">Nothing yet.</p>
      ) : (
        <ol className="agents-dialog__activity" aria-labelledby={id}>
          {activity.map(({ id: key, call }) => (
            <li
              key={key}
              className={`agents-dialog__call${call.error ? " agents-dialog__call--failed" : ""}`}
            >
              <span className="agents-dialog__tool">{call.tool}</span>
              {call.target ? <span>{call.target}</span> : null}
              {call.error ? <span>failed: {call.error}</span> : null}
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}
