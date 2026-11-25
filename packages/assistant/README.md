# @repo/assistant

**Status:** AI assistant.

## Purpose

The AI assistant's window and its setup, through the `AssistantPort` the
app gives it (`ide-assistant` behind it).

- `SetupDialog`: File → Set Up the AI Assistant… (`assistant.setup`). A step
  guide in a nuvui `Dialog` with a `Stepper`:
  1. **Provider:** Anthropic, OpenAI, Azure AI Foundry, Gemini, Ollama or
     Amazon Bedrock, as cards.
  2. **Connect:** what that provider needs. An API key (kept in the
     system's credential store; "Saved" when one is kept already), Azure's
     endpoint, Ollama's address, Bedrock's region and either an AWS profile
     or a Bedrock API key.
  3. **Model:** the provider's models, listed with the key typed, to pick
     from; or typed (Azure's deployment name, a Bedrock inference profile).
  4. **Check:** asks the model for a word, and shows its answer or why it
     failed. Save keeps the setup and opens the assistant.

  "Forget the setup" removes it and every key kept. Each step's heading
  takes focus, for a screen reader.
- `AssistantPanel`: the assistant, for the shell's secondary side bar.
  - The conversation, with `@nuvui/chat`: your messages, the replies as
    Markdown as they stream (links aren't followed: their address is shown
    beside them), the tools it used as cards that open to what was asked
    and answered, and each change it proposes as a diff (`renderDiff`, from
    the app) with Accept and Reject.
  - The prompt box (Enter sends; Stop while a reply comes), suggestions to
    start from, a new chat, and the setup.
  - It registers `assistant.focus` (opens the side bar and the prompt box)
    and `assistant.newChat`. The file in front of the person
    (`activePath`) is sent with each message.
- `createAssistantStore(port, bus)`: the setup's status and the
  conversation, built from `assistant.event` on the bus.
- `PROVIDERS`, `providerInfo`, `needsKey`, `emptySetup`, `describeSetup`:
  what the setup says about each provider.

## Public API

- `SetupDialog` with `{ store, port, registry, onSaved? }`.
- `AssistantPanel` with `{ store, registry, renderDiff, activePath? }`.
- `createAssistantStore`, types `AssistantStore`, `AssistantState`, `Entry`;
  `describeTool`; the provider helpers above.

## May depend on

`@repo/contracts` (`AssistantPort`, the DTOs, `AppEvents`), `@repo/kernel`,
`@repo/file-icons`, `react`, `@nuvui/react`, `@nuvui/chat`, `@nuvui/icons`.
Never another feature package, `@repo/ipc` or `@tauri-apps/*`.
