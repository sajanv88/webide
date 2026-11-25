import "./setup-dialog.scss";
import {
  Alert,
  AlertDescription,
  AlertTitle,
  Button,
  ChoiceCardDescription,
  ChoiceCardTitle,
  Combobox,
  ComboboxContent,
  ComboboxEmpty,
  ComboboxItem,
  ComboboxTrigger,
  ComboboxValue,
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogTitle,
  Field,
  FieldControl,
  FieldDescription,
  FieldError,
  FieldLabel,
  Input,
  PasswordInput,
  RadioCard,
  RadioGroup,
  RadioGroupItem,
  Spinner,
  Stepper,
  StepperContent,
  StepperIndicator,
  StepperItem,
  StepperTitle,
} from "@nuvui/react";
import type {
  AppCommandId,
  AssistantPort,
  AssistantSetupArgs,
  BedrockAuthDto,
  ModelInfoDto,
  ProviderKindDto,
  ProviderSetupDto,
} from "@repo/contracts";
import type { CommandRegistry } from "@repo/kernel";
import { useEffect, useId, useRef, useState, useSyncExternalStore } from "react";
import type { AssistantStore } from "./assistant-store";
import {
  BEDROCK_DEFAULT_REGION,
  emptySetup,
  needsKey,
  OLLAMA_DEFAULT,
  PROVIDERS,
  providerInfo,
} from "./providers";

export interface SetupDialogProps {
  readonly store: AssistantStore;
  readonly port: AssistantPort;
  readonly registry: CommandRegistry<AppCommandId>;
  /** After a save: the app shows the assistant. */
  readonly onSaved?: () => void;
}

const STEPS = ["Provider", "Connect", "Model", "Check"] as const;

/**
 * File → Set Up the AI Assistant…: a step guide. The provider, how to
 * reach it (a key, an endpoint, a region or profile), the model, and a
 * check that it answers. It registers `assistant.setup`.
 */
export function SetupDialog({ store, port, registry, onSaved }: SetupDialogProps) {
  const [open, setOpen] = useState(false);
  useEffect(
    () =>
      registry.register({
        id: "assistant.setup",
        title: "Set Up the AI Assistant…",
        category: "Assistant",
        run: () => setOpen(true),
      }),
    [registry],
  );
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent size="lg" className="assistant-setup">
        <DialogTitle>Set up the AI assistant</DialogTitle>
        <DialogDescription>
          Requests go from the app to the provider you choose; keys stay in this computer's
          credential store.
        </DialogDescription>
        {open ? (
          <SetupSteps
            store={store}
            port={port}
            onDone={() => {
              setOpen(false);
              onSaved?.();
            }}
          />
        ) : null}
      </DialogContent>
    </Dialog>
  );
}

type Models =
  | { readonly status: "idle" }
  | { readonly status: "loading" }
  | { readonly status: "ready"; readonly models: readonly ModelInfoDto[] }
  | { readonly status: "failed"; readonly message: string };

type Check =
  | { readonly status: "idle" }
  | { readonly status: "checking" }
  | { readonly status: "passed"; readonly reply: string }
  | { readonly status: "failed"; readonly message: string };

function SetupSteps({
  store,
  port,
  onDone,
}: {
  readonly store: AssistantStore;
  readonly port: AssistantPort;
  readonly onDone: () => void;
}) {
  const { status } = useSyncExternalStore(store.subscribe, store.getState);
  const saved = status?.setup ?? null;
  const [step, setStep] = useState(1);
  const [setup, setSetup] = useState<ProviderSetupDto>(saved ?? emptySetup("anthropic"));
  const [key, setKey] = useState("");
  const [errors, setErrors] = useState<{ endpoint?: string; key?: string; model?: string }>({});
  const [models, setModels] = useState<Models>({ status: "idle" });
  const [check, setCheck] = useState<Check>({ status: "idle" });
  const [failure, setFailure] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const heading = useRef<HTMLHeadingElement>(null);
  const ids = useId();

  const info = providerInfo(setup.kind);
  const asksKey = needsKey(setup.kind, setup.bedrockAuth);
  // A key kept for this provider can be left as it is.
  const keyKept = saved?.kind === setup.kind && (status?.hasSecret ?? false);
  const args = (): AssistantSetupArgs => ({
    setup,
    ...(asksKey && key.trim() ? { secret: key.trim() } : {}),
  });
  const change = (next: Partial<ProviderSetupDto>) => {
    setSetup((now) => ({ ...now, ...next }));
    setCheck({ status: "idle" });
  };

  // The new step's heading takes focus, for a screen reader.
  // biome-ignore lint/correctness/useExhaustiveDependencies: runs when the step changes
  useEffect(() => {
    heading.current?.focus();
  }, [step]);

  const loadModels = async () => {
    if (setup.kind === "azure") {
      setModels({ status: "ready", models: [] });
      return;
    }
    setModels({ status: "loading" });
    const result = await port.models(args());
    setModels(
      result.ok
        ? { status: "ready", models: result.value }
        : { status: "failed", message: result.error.message },
    );
  };

  const next = () => {
    if (step === 2) {
      const found: typeof errors = {};
      const endpoint = setup.endpoint?.trim() ?? "";
      if (setup.kind === "azure" && !endpoint.startsWith("https://")) {
        found.endpoint = "Enter the resource's endpoint, starting with https://.";
      }
      if (setup.kind === "ollama" && endpoint && !/^https?:\/\//.test(endpoint)) {
        found.endpoint = "The address starts with http:// or https://.";
      }
      if (asksKey && !key.trim() && !keyKept) found.key = "Enter the API key.";
      setErrors(found);
      if (found.endpoint || found.key) return;
      void loadModels();
    }
    if (step === 3) {
      if (!setup.model.trim()) {
        setErrors({ model: `Enter the ${info.modelLabel.toLowerCase()}.` });
        return;
      }
      setErrors({});
    }
    setStep(step + 1);
  };

  const runCheck = async () => {
    setCheck({ status: "checking" });
    const result = await port.check(args());
    setCheck(
      result.ok
        ? { status: "passed", reply: result.value }
        : { status: "failed", message: result.error.message },
    );
  };

  const save = async () => {
    setBusy(true);
    setFailure(null);
    const result = await port.save(args());
    setBusy(false);
    if (!result.ok) {
      setFailure(result.error.message);
      return;
    }
    store.setStatus(result.value);
    onDone();
  };

  const forget = async () => {
    setBusy(true);
    const result = await port.forget();
    setBusy(false);
    if (!result.ok) {
      setFailure(result.error.message);
      return;
    }
    store.setStatus(result.value);
    setSetup(emptySetup("anthropic"));
    setKey("");
    setStep(1);
  };

  return (
    <div className="assistant-setup__body">
      <Stepper value={step} aria-label="Setting up the assistant">
        {STEPS.map((title, index) => (
          <StepperItem key={title} step={index + 1}>
            <StepperIndicator />
            <StepperContent>
              <StepperTitle>{title}</StepperTitle>
            </StepperContent>
          </StepperItem>
        ))}
      </Stepper>

      {failure ? (
        <Alert intent="danger">
          <AlertTitle>The setup wasn't saved</AlertTitle>
          <AlertDescription>{failure}</AlertDescription>
        </Alert>
      ) : null}

      <section className="assistant-setup__step" aria-labelledby={`${ids}-heading`}>
        <h3 id={`${ids}-heading`} ref={heading} tabIndex={-1} className="assistant-setup__heading">
          {step === 1
            ? "Where's the model?"
            : step === 2
              ? `Connect to ${info.name}`
              : step === 3
                ? "Which model?"
                : "Check that it answers"}
        </h3>

        {step === 1 ? (
          <RadioGroup
            className="assistant-setup__providers"
            aria-labelledby={`${ids}-heading`}
            value={setup.kind}
            onValueChange={(value) => {
              const kind = value as ProviderKindDto;
              setSetup(saved?.kind === kind ? saved : emptySetup(kind));
              setKey("");
              setModels({ status: "idle" });
              setCheck({ status: "idle" });
            }}
          >
            {PROVIDERS.map((provider) => (
              <RadioCard key={provider.kind} value={provider.kind}>
                <ChoiceCardTitle>{provider.name}</ChoiceCardTitle>
                <ChoiceCardDescription>{provider.description}</ChoiceCardDescription>
              </RadioCard>
            ))}
          </RadioGroup>
        ) : null}

        {step === 2 ? (
          <div className="assistant-setup__fields">
            {setup.kind === "azure" ? (
              <Field controlId={`${ids}-endpoint`}>
                <FieldLabel>Endpoint</FieldLabel>
                <FieldControl>
                  <Input
                    value={setup.endpoint ?? ""}
                    onChange={(event) => change({ endpoint: event.target.value })}
                    placeholder="https://your-resource.openai.azure.com"
                    spellCheck={false}
                  />
                </FieldControl>
                <FieldDescription>
                  From the Azure portal, under Keys and Endpoint. The v1 API is used.
                </FieldDescription>
                <FieldError>{errors.endpoint}</FieldError>
              </Field>
            ) : null}

            {setup.kind === "ollama" ? (
              <Field controlId={`${ids}-endpoint`}>
                <FieldLabel>Ollama's address</FieldLabel>
                <FieldControl>
                  <Input
                    value={setup.endpoint ?? ""}
                    onChange={(event) => change({ endpoint: event.target.value })}
                    placeholder={OLLAMA_DEFAULT}
                    spellCheck={false}
                  />
                </FieldControl>
                <FieldDescription>
                  Leave it empty for Ollama on this computer. Start it with ollama serve.
                </FieldDescription>
                <FieldError>{errors.endpoint}</FieldError>
              </Field>
            ) : null}

            {setup.kind === "bedrock" ? (
              <>
                <Field controlId={`${ids}-region`}>
                  <FieldLabel>Region</FieldLabel>
                  <FieldControl>
                    <Input
                      value={setup.region ?? ""}
                      onChange={(event) => change({ region: event.target.value })}
                      placeholder={BEDROCK_DEFAULT_REGION}
                      spellCheck={false}
                    />
                  </FieldControl>
                </Field>
                <Field>
                  <FieldLabel asChild>
                    <span>Sign in with</span>
                  </FieldLabel>
                  <FieldControl>
                    <RadioGroup
                      className="assistant-setup__choices"
                      orientation="horizontal"
                      value={setup.bedrockAuth}
                      onValueChange={(value) => change({ bedrockAuth: value as BedrockAuthDto })}
                    >
                      <div className="assistant-setup__choice">
                        <RadioGroupItem value="profile" id={`${ids}-profile-auth`} />
                        <label htmlFor={`${ids}-profile-auth`}>My AWS profile</label>
                      </div>
                      <div className="assistant-setup__choice">
                        <RadioGroupItem value="apiKey" id={`${ids}-key-auth`} />
                        <label htmlFor={`${ids}-key-auth`}>A Bedrock API key</label>
                      </div>
                    </RadioGroup>
                  </FieldControl>
                </Field>
                {setup.bedrockAuth === "profile" ? (
                  <Field controlId={`${ids}-profile`}>
                    <FieldLabel>Profile</FieldLabel>
                    <FieldControl>
                      <Input
                        value={setup.profile ?? ""}
                        onChange={(event) => change({ profile: event.target.value })}
                        placeholder="default"
                        spellCheck={false}
                      />
                    </FieldControl>
                    <FieldDescription>
                      From the AWS CLI's configuration. For SSO, sign in first with aws sso login.
                    </FieldDescription>
                  </Field>
                ) : null}
              </>
            ) : null}

            {asksKey ? (
              <Field controlId={`${ids}-key`}>
                <FieldLabel>API key</FieldLabel>
                <FieldControl>
                  <PasswordInput
                    value={key}
                    onChange={(event) => {
                      setKey(event.target.value);
                      setCheck({ status: "idle" });
                    }}
                    placeholder={keyKept ? "Saved: leave empty to keep it" : ""}
                    autoComplete="new-password"
                    showLabel="Show the key"
                    hideLabel="Hide the key"
                    spellCheck={false}
                  />
                </FieldControl>
                <FieldDescription>
                  {info.keyHelp} It's kept in this computer's credential store, never in a file.
                </FieldDescription>
                <FieldError>{errors.key}</FieldError>
              </Field>
            ) : null}
          </div>
        ) : null}

        {step === 3 ? (
          <div className="assistant-setup__fields">
            {models.status === "loading" ? (
              <div className="assistant-setup__waiting">
                <Spinner label={`Asking ${info.name} for its models`} />
              </div>
            ) : null}
            {models.status === "failed" ? (
              <Alert intent="warning">
                <AlertTitle>The models couldn't be listed</AlertTitle>
                <AlertDescription>
                  {models.message} You can still type the model's id below.
                </AlertDescription>
              </Alert>
            ) : null}
            {models.status === "ready" && models.models.length > 0 ? (
              <Field>
                <FieldLabel asChild>
                  <span>{info.name}'s models</span>
                </FieldLabel>
                <FieldControl>
                  <Combobox value={setup.model} onValueChange={(model) => change({ model })}>
                    <ComboboxTrigger className="assistant-setup__models">
                      <ComboboxValue placeholder="Choose a model" />
                    </ComboboxTrigger>
                    <ComboboxContent label="Find a model" searchPlaceholder="Find a model">
                      <ComboboxEmpty>No model matches.</ComboboxEmpty>
                      {models.models.map((model) => (
                        <ComboboxItem
                          key={model.id}
                          value={model.id}
                          keywords={model.name ? [model.name] : []}
                        >
                          {model.name && model.name !== model.id
                            ? `${model.name} (${model.id})`
                            : model.id}
                        </ComboboxItem>
                      ))}
                    </ComboboxContent>
                  </Combobox>
                </FieldControl>
              </Field>
            ) : null}
            <Field controlId={`${ids}-model`}>
              <FieldLabel>{info.modelLabel}</FieldLabel>
              <FieldControl>
                <Input
                  value={setup.model}
                  onChange={(event) => change({ model: event.target.value })}
                  placeholder={info.modelExample}
                  spellCheck={false}
                />
              </FieldControl>
              <FieldDescription>
                {setup.kind === "azure"
                  ? "The name you gave the deployment in Azure AI Foundry."
                  : "Chosen above, or typed. It has to be able to call tools to read the folder."}
              </FieldDescription>
              <FieldError>{errors.model}</FieldError>
            </Field>
          </div>
        ) : null}

        {step === 4 ? (
          <div className="assistant-setup__fields">
            <p className="assistant-setup__summary">
              {info.name}, {setup.model}
              {setup.kind === "bedrock" ? `, in ${setup.region || BEDROCK_DEFAULT_REGION}` : ""}.
            </p>
            <div>
              <Button
                intent="secondary"
                disabled={check.status === "checking"}
                onClick={() => void runCheck()}
              >
                {check.status === "checking" ? "Checking…" : "Check the connection"}
              </Button>
            </div>
            {check.status === "checking" ? (
              <Spinner label="Waiting for the model's answer" />
            ) : null}
            {check.status === "passed" ? (
              <Alert intent="success">
                <AlertTitle>It answers</AlertTitle>
                <AlertDescription>The model said: “{check.reply}”</AlertDescription>
              </Alert>
            ) : null}
            {check.status === "failed" ? (
              <Alert intent="danger">
                <AlertTitle>It didn't answer</AlertTitle>
                <AlertDescription>{check.message}</AlertDescription>
              </Alert>
            ) : null}
          </div>
        ) : null}
      </section>

      <DialogFooter className="assistant-setup__footer">
        {saved && step === 1 ? (
          <Button intent="ghost" disabled={busy} onClick={() => void forget()}>
            Forget the setup
          </Button>
        ) : null}
        <DialogClose asChild>
          <Button intent="secondary">Cancel</Button>
        </DialogClose>
        {step > 1 ? (
          <Button intent="secondary" onClick={() => setStep(step - 1)}>
            Back
          </Button>
        ) : null}
        {step < STEPS.length ? (
          <Button onClick={next}>Next</Button>
        ) : (
          <Button disabled={busy} onClick={() => void save()}>
            {busy ? "Saving…" : "Save"}
          </Button>
        )}
      </DialogFooter>
    </div>
  );
}
