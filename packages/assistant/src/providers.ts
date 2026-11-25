import type { BedrockAuthDto, ProviderKindDto, ProviderSetupDto } from "@repo/contracts";

/** What the setup says about each provider, and what it asks for. */
export interface ProviderInfo {
  readonly kind: ProviderKindDto;
  readonly name: string;
  /** One line, on its card. */
  readonly description: string;
  /** Where the person gets a key, in words: no link leaves the app. */
  readonly keyHelp?: string;
  /** What the model field is called. */
  readonly modelLabel: string;
  /** An example for the model field. */
  readonly modelExample: string;
}

export const PROVIDERS: readonly ProviderInfo[] = [
  {
    kind: "anthropic",
    name: "Anthropic",
    description: "Claude, with an API key from the Claude Console.",
    keyHelp: "Make one in the Claude Console (console.anthropic.com), under API keys.",
    modelLabel: "Model",
    modelExample: "claude-sonnet-4-5",
  },
  {
    kind: "openai",
    name: "OpenAI",
    description: "GPT models, with an API key from the OpenAI platform.",
    keyHelp: "Make one on the OpenAI platform (platform.openai.com), under API keys.",
    modelLabel: "Model",
    modelExample: "gpt-5",
  },
  {
    kind: "azure",
    name: "Azure AI Foundry",
    description: "A model deployed in your Azure AI Foundry or Azure OpenAI resource.",
    keyHelp: "The resource's key is in the Azure portal, under Keys and Endpoint.",
    modelLabel: "Deployment name",
    modelExample: "gpt-5-deployment",
  },
  {
    kind: "gemini",
    name: "Gemini",
    description: "Google's Gemini models, with an API key from Google AI Studio.",
    keyHelp: "Make one in Google AI Studio (aistudio.google.com), under Get API key.",
    modelLabel: "Model",
    modelExample: "gemini-2.5-pro",
  },
  {
    kind: "ollama",
    name: "Ollama",
    description: "Models running on this computer or your network. Nothing leaves it.",
    modelLabel: "Model",
    modelExample: "qwen3-coder",
  },
  {
    kind: "bedrock",
    name: "Amazon Bedrock",
    description: "Models in your AWS account, with your AWS profile or a Bedrock API key.",
    keyHelp: "Make one in the Bedrock console, under API keys.",
    modelLabel: "Model or inference profile",
    modelExample: "us.anthropic.claude-sonnet-4-5-20250929-v1:0",
  },
];

export const providerInfo = (kind: ProviderKindDto): ProviderInfo =>
  PROVIDERS.find((provider) => provider.kind === kind) ?? (PROVIDERS[0] as ProviderInfo);

export const OLLAMA_DEFAULT = "http://127.0.0.1:11434";
export const BEDROCK_DEFAULT_REGION = "us-east-1";

/** Whether this setup asks for a key. */
export const needsKey = (kind: ProviderKindDto, bedrockAuth: BedrockAuthDto): boolean =>
  kind === "ollama" ? false : kind === "bedrock" ? bedrockAuth === "apiKey" : true;

/** A setup with nothing chosen yet but the provider. */
export const emptySetup = (kind: ProviderKindDto): ProviderSetupDto => ({
  kind,
  model: "",
  endpoint: null,
  region: null,
  profile: null,
  bedrockAuth: "profile",
});

/** The provider and the model, for a heading: "Anthropic · claude-sonnet-4-5". */
export const describeSetup = (setup: ProviderSetupDto): string =>
  `${providerInfo(setup.kind).name} · ${setup.model}`;
