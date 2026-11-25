import "./assistant-panel.scss";
import {
  Conversation,
  ConversationContent,
  ConversationScrollButton,
  Message,
  MessageContent,
  MessageLoader,
  PromptInput,
  PromptInputSubmit,
  PromptInputTextarea,
  PromptInputToolbar,
  PromptSuggestion,
  PromptSuggestions,
  ToolCall,
  ToolCallApproval,
  ToolCallContent,
  ToolCallError,
  ToolCallInput,
  ToolCallOutput,
  ToolCallTrigger,
} from "@nuvui/chat";
import { Markdown } from "@nuvui/chat/markdown";
import { PlusIcon, SettingsIcon } from "@nuvui/icons";
import {
  Alert,
  AlertDescription,
  Button,
  Empty,
  EmptyActions,
  EmptyDescription,
  EmptyTitle,
} from "@nuvui/react";
import type { AppCommandId } from "@repo/contracts";
import { FileTypeIcon } from "@repo/file-icons";
import type { CommandRegistry } from "@repo/kernel";
import {
  type ComponentProps,
  type ReactNode,
  useEffect,
  useMemo,
  useRef,
  useSyncExternalStore,
} from "react";
import { type AssistantStore, describeTool, type Entry } from "./assistant-store";
import { describeSetup } from "./providers";

export interface AssistantPanelProps {
  readonly store: AssistantStore;
  readonly registry: CommandRegistry<AppCommandId>;
  /** The file in front of the person, relative to the workspace, when there's one. */
  readonly activePath?: () => string | undefined;
  /** Draws a proposed change: the file before and after. */
  readonly renderDiff: (path: string, original: string, proposed: string) => ReactNode;
}

const SUGGESTIONS = [
  "What does this project do?",
  "Explain the file I have open",
  "Find where errors are handled",
] as const;

/**
 * A link in a reply isn't followed: the app's window would leave the app.
 * Its address is shown beside its text instead.
 */
const MARKDOWN_COMPONENTS = {
  a: ({ href, children }: ComponentProps<"a">) => (
    <span className="assistant__link" title={href}>
      {children}
      {href && href !== children ? ` (${href})` : null}
    </span>
  ),
};

/**
 * The assistant, in the secondary side bar: the conversation (the model's
 * replies as Markdown, the tools it used, the changes it proposes, each
 * shown as a diff to accept or reject) and the prompt box. It registers
 * `assistant.focus` and `assistant.newChat`.
 */
export function AssistantPanel({ store, registry, activePath, renderDiff }: AssistantPanelProps) {
  const state = useSyncExternalStore(store.subscribe, store.getState);
  const prompt = useRef<HTMLDivElement>(null);
  const setup = state.status?.setup ?? null;

  useEffect(() => {
    const disposers = [
      registry.register({
        id: "assistant.focus",
        title: "Open the AI Assistant",
        category: "Assistant",
        run: async () => {
          await registry.execute("view.showAside");
          // Once it's drawn: the pane may have just opened.
          requestAnimationFrame(() => prompt.current?.querySelector("textarea")?.focus());
        },
      }),
      registry.register({
        id: "assistant.newChat",
        title: "New Chat",
        category: "Assistant",
        run: () => store.newChat(),
      }),
    ];
    return () => {
      for (const dispose of disposers) dispose();
    };
  }, [registry, store]);

  const send = (text: string) => void store.send(text, activePath?.());
  const last = state.entries.at(-1);
  // Waiting for the first word of a reply, or for the next after a tool.
  const waiting =
    state.replying &&
    !(last?.kind === "assistant" && last.streaming) &&
    !(last?.kind === "proposal" && last.state === "waiting");

  return (
    <div className="assistant">
      <div className="assistant__header">
        <h2 className="assistant__title">Assistant</h2>
        {setup ? <span className="assistant__model">{describeSetup(setup)}</span> : null}
        <div className="assistant__tools">
          <Button
            intent="ghost"
            size="sm"
            className="assistant__icon"
            aria-label="New chat"
            title="New chat"
            disabled={!setup}
            onClick={() => void registry.execute("assistant.newChat")}
          >
            <PlusIcon size={16} />
          </Button>
          <Button
            intent="ghost"
            size="sm"
            className="assistant__icon"
            aria-label="Set up the assistant"
            title="Set up the assistant"
            onClick={() => void registry.execute("assistant.setup")}
          >
            <SettingsIcon size={16} />
          </Button>
        </div>
      </div>

      {state.failure ? (
        <Alert intent="danger" className="assistant__alert">
          <AlertDescription>{state.failure}</AlertDescription>
        </Alert>
      ) : null}

      {state.status && !setup ? (
        <Empty className="assistant__empty">
          <EmptyTitle>Set up the assistant</EmptyTitle>
          <EmptyDescription>
            Choose a provider (Anthropic, OpenAI, Azure AI Foundry, Gemini, Ollama or Amazon
            Bedrock) and a model. It reads the open folder, and every change it proposes waits for
            you to accept it.
          </EmptyDescription>
          <EmptyActions>
            <Button onClick={() => void registry.execute("assistant.setup")}>Set up</Button>
          </EmptyActions>
        </Empty>
      ) : null}

      {setup ? (
        <>
          <Conversation className="assistant__conversation">
            <ConversationContent aria-label="Conversation with the assistant">
              {state.entries.length === 0 ? (
                <PromptSuggestions className="assistant__suggestions">
                  {SUGGESTIONS.map((suggestion) => (
                    <PromptSuggestion key={suggestion} onClick={() => send(suggestion)}>
                      {suggestion}
                    </PromptSuggestion>
                  ))}
                </PromptSuggestions>
              ) : null}
              {state.entries.map((entry) => (
                <EntryView key={entry.id} entry={entry} store={store} renderDiff={renderDiff} />
              ))}
              {waiting ? (
                <Message from="assistant" streaming>
                  <MessageLoader />
                </Message>
              ) : null}
            </ConversationContent>
            <ConversationScrollButton />
          </Conversation>
          <div ref={prompt} className="assistant__prompt">
            <PromptInput
              busy={state.replying}
              onStop={() => void store.stop()}
              onSubmit={(text) => send(text)}
            >
              <PromptInputTextarea
                aria-label="Message the assistant"
                placeholder="Ask about the code, or ask for a change"
              />
              <PromptInputToolbar>
                <PromptInputSubmit />
              </PromptInputToolbar>
            </PromptInput>
          </div>
        </>
      ) : null}
    </div>
  );
}

function EntryView({
  entry,
  store,
  renderDiff,
}: {
  readonly entry: Entry;
  readonly store: AssistantStore;
  readonly renderDiff: AssistantPanelProps["renderDiff"];
}) {
  switch (entry.kind) {
    case "user":
      return (
        <Message from="user">
          <MessageContent>{entry.text}</MessageContent>
        </Message>
      );
    case "assistant":
      return (
        <Message from="assistant" streaming={entry.streaming}>
          <MessageContent className="assistant__reply">
            <Markdown
              streaming={entry.streaming}
              headingOffset={2}
              components={MARKDOWN_COMPONENTS}
            >
              {entry.text}
            </Markdown>
          </MessageContent>
        </Message>
      );
    case "tool":
      return <ToolView entry={entry} />;
    case "proposal":
      return <ProposalView entry={entry} store={store} renderDiff={renderDiff} />;
    case "note":
      return entry.failed ? (
        <Alert intent="danger" className="assistant__alert">
          <AlertDescription>{entry.text}</AlertDescription>
        </Alert>
      ) : (
        <p className="assistant__note">{entry.text}</p>
      );
  }
}

function ToolView({ entry }: { readonly entry: Extract<Entry, { kind: "tool" }> }) {
  const input = useMemo(() => {
    try {
      return JSON.parse(entry.input) as unknown;
    } catch {
      return entry.input;
    }
  }, [entry.input]);
  return (
    <ToolCall status={entry.status} className="assistant__tool">
      <ToolCallTrigger>{describeTool(entry.name, entry.input)}</ToolCallTrigger>
      <ToolCallContent>
        <ToolCallInput value={input} />
        {entry.status === "done" && entry.output !== null ? (
          <ToolCallOutput value={entry.output} />
        ) : null}
        {entry.status === "error" ? <ToolCallError>{entry.output}</ToolCallError> : null}
      </ToolCallContent>
    </ToolCall>
  );
}

const PROPOSAL_STATUS = {
  waiting: "approval",
  deciding: "running",
  accepted: "done",
  rejected: "denied",
  failed: "error",
} as const;

const PROPOSAL_LABEL = {
  waiting: "Waiting for you",
  deciding: "Writing",
  accepted: "Accepted",
  rejected: "Rejected",
  failed: "Failed",
} as const;

function ProposalView({
  entry,
  store,
  renderDiff,
}: {
  readonly entry: Extract<Entry, { kind: "proposal" }>;
  readonly store: AssistantStore;
  readonly renderDiff: AssistantPanelProps["renderDiff"];
}) {
  const created = entry.original === null;
  return (
    <ToolCall status={PROPOSAL_STATUS[entry.state]} defaultOpen className="assistant__proposal">
      <ToolCallTrigger statusLabel={PROPOSAL_LABEL[entry.state]}>
        <span className="assistant__file">
          <FileTypeIcon name={entry.path} />
          {created ? `Create ${entry.path}` : `Change ${entry.path}`}
        </span>
      </ToolCallTrigger>
      <ToolCallContent>
        {entry.summary ? <p className="assistant__summary">{entry.summary}</p> : null}
        <div className="assistant__diff">
          {renderDiff(entry.path, entry.original ?? "", entry.proposed)}
        </div>
        {entry.error ? <ToolCallError>{entry.error}</ToolCallError> : null}
      </ToolCallContent>
      <ToolCallApproval
        approveLabel="Accept"
        denyLabel="Reject"
        onApprove={() => void store.decide(entry.id, true)}
        onDeny={() => void store.decide(entry.id, false)}
      >
        {created ? `Create ${entry.path} with this text?` : `Write this change to ${entry.path}?`}
      </ToolCallApproval>
    </ToolCall>
  );
}
