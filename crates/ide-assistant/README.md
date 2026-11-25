# ide-assistant

**Status:** AI assistant.

## Purpose

The IDE's own AI assistant: conversations with a model the person sets up,
which read the open folder and propose changes the person accepts or
rejects.

- **Six providers**, each in its own API, streamed, with tools:
  - Anthropic: the Messages API, with an API key.
  - OpenAI: Chat Completions, with an API key.
  - Azure AI Foundry: the v1 API (OpenAI's shape) at the resource's
    endpoint, with its key. The model is the deployment's name.
  - Gemini: `streamGenerateContent`, with an API key. Thought signatures
    are sent back as they came, as Gemini needs for tool calls.
  - Ollama: its OpenAI-compatible API, on this machine
    (`http://127.0.0.1:11434`) or another address. No key. A model that
    can't use tools answers without them, and says so.
  - Amazon Bedrock: ConverseStream, signed with SigV4 from the AWS
    credentials on this machine (a profile, SSO included, through
    aws-config's default chain), or with a Bedrock API key. Its event stream
    is decoded and CRC-checked here.
- **From this process.** Every request goes from the app (reqwest, rustls);
  the webview never reaches the network, so the CSP stays as it is.
- **Keys** are kept in the system's credential store (Windows Credential
  Manager, the macOS Keychain, the Secret Service on Linux), under
  `webide-assistant`, one per provider. They're never in a file of the
  app's, never logged, and never sent back to the webview. The setup
  (provider, model, endpoint, region, profile) is in `assistant.json`
  beside the settings.
- **Reading the folder** goes through `WorkspaceTools` (`ide-core`), the
  read tools agents get over MCP (`ide-mcp` implements it), so the same
  refusals hold: nothing outside the folder, nothing in `.git`, the size
  limits from the settings.
- **Changes wait for the person.** `propose_edit` (one occurrence of
  `old_text` replaced, or a whole file) is shown as a diff; it's written
  through the same port only when the person accepts it. A rejection is
  told to the model. Nothing runs a command.
- **A conversation** keeps its messages in memory until it's cleared. One
  reply at a time; stopping it rejects a change that waits. A reply that
  fails keeps what the person saw.

## Public API

- `AssistantService::new(config_dir)` (implements
  `ide_core::assistant::AssistantHost`): `status`, `save` (with the key),
  `forget`, `list_models`, `check` (asks for a word), `send` (the reply
  comes as `AssistantEvent`s), `stop`, `decide`, `clear`.

## May depend on

`ide-core`, `reqwest` (rustls), `keyring`, `aws-config`, `aws-sigv4`,
`aws-credential-types`, `tokio`, `serde`, `serde_json`. Never another
workspace crate or Tauri. Checked by `cargo xtask check-boundaries`.
