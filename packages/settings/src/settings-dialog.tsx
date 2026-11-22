import "./settings-dialog.scss";
import {
  Alert,
  AlertDescription,
  AlertTitle,
  Button,
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
  NumberField,
  RadioGroup,
  RadioGroupItem,
  Spinner,
} from "@nuvui/react";
import type { AppCommandId, AppEvents, SettingsDto, SettingsPort, ThemeDto } from "@repo/contracts";
import type { CommandRegistry, EventBus } from "@repo/kernel";
import { type FormEvent, useEffect, useState } from "react";

export interface SettingsDialogProps {
  readonly port: SettingsPort;
  readonly registry: CommandRegistry<AppCommandId>;
  /** `settings.changed` goes here after a save. */
  readonly bus: EventBus<AppEvents>;
}

const MIB = 1024 * 1024;
/** The size fields, in megabytes (MiB). The backend checks the same limits in bytes. */
const MIN_MB = 0.1;
const MAX_MB = 256;

const THEMES: readonly { readonly value: ThemeDto; readonly label: string }[] = [
  { value: "system", label: "Same as the system" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

const toMb = (bytes: number): number => Math.round((bytes / MIB) * 100) / 100;
const toBytes = (mb: number): number => Math.round(mb * MIB);

/**
 * File → Settings…: the theme, and the size limits for opening files. It
 * registers `settings.open`, which opens it. Saving writes through the port
 * and announces `settings.changed`.
 */
export function SettingsDialog({ port, registry, bus }: SettingsDialogProps) {
  const [open, setOpen] = useState(false);

  useEffect(
    () =>
      registry.register({
        id: "settings.open",
        title: "Settings…",
        category: "Preferences",
        run: () => setOpen(true),
      }),
    [registry],
  );

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent size="md">
        <DialogTitle>Settings</DialogTitle>
        <DialogDescription>Saved for this computer, for every folder.</DialogDescription>
        {/* Read afresh each time it opens. */}
        {open ? (
          <SettingsForm
            port={port}
            onSaved={(settings) => {
              bus.emit("settings.changed", settings);
              setOpen(false);
            }}
          />
        ) : null}
      </DialogContent>
    </Dialog>
  );
}

type Loaded =
  | { readonly status: "loading" }
  | { readonly status: "failed"; readonly message: string }
  | { readonly status: "ready"; readonly settings: SettingsDto };

interface FormProps {
  readonly port: SettingsPort;
  readonly onSaved: (settings: SettingsDto) => void;
}

function SettingsForm({ port, onSaved }: FormProps) {
  const [loaded, setLoaded] = useState<Loaded>({ status: "loading" });

  useEffect(() => {
    let live = true;
    void port.get().then((result) => {
      if (!live) return;
      setLoaded(
        result.ok
          ? { status: "ready", settings: result.value }
          : { status: "failed", message: result.error.message },
      );
    });
    return () => {
      live = false;
    };
  }, [port]);

  switch (loaded.status) {
    case "loading":
      return (
        <div className="settings-dialog__loading">
          <Spinner label="Reading the settings" />
        </div>
      );
    case "failed":
      return (
        <>
          <Alert intent="danger">
            <AlertTitle>Couldn't read the settings</AlertTitle>
            <AlertDescription>{loaded.message}</AlertDescription>
          </Alert>
          <DialogFooter>
            <DialogClose asChild>
              <Button intent="secondary">Close</Button>
            </DialogClose>
          </DialogFooter>
        </>
      );
    case "ready":
      return <ReadyForm port={port} initial={loaded.settings} onSaved={onSaved} />;
  }
}

function ReadyForm({ port, initial, onSaved }: FormProps & { readonly initial: SettingsDto }) {
  const [theme, setTheme] = useState<ThemeDto>(initial.theme);
  const [readOnlyMb, setReadOnlyMb] = useState<number | null>(toMb(initial.readOnlyAboveBytes));
  const [refuseMb, setRefuseMb] = useState<number | null>(toMb(initial.refuseAboveBytes));
  const [errors, setErrors] = useState<{ readOnly?: string; refuse?: string }>({});
  const [failure, setFailure] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const next: { readOnly?: string; refuse?: string } = {};
    if (readOnlyMb === null) next.readOnly = "Enter a size in megabytes.";
    if (refuseMb === null) next.refuse = "Enter a size in megabytes.";
    if (readOnlyMb !== null && refuseMb !== null && readOnlyMb > refuseMb) {
      next.refuse = `Enter at least ${readOnlyMb} MB, the read-only size above.`;
    }
    setErrors(next);
    if (next.readOnly || next.refuse || readOnlyMb === null || refuseMb === null) {
      const first = event.currentTarget.querySelector<HTMLElement>(
        next.readOnly ? "#settings-read-only" : "#settings-refuse",
      );
      first?.focus();
      return;
    }
    setSaving(true);
    setFailure(null);
    const result = await port.update({
      theme,
      readOnlyAboveBytes: toBytes(readOnlyMb),
      refuseAboveBytes: toBytes(refuseMb),
    });
    setSaving(false);
    if (!result.ok) {
      setFailure(result.error.message);
      return;
    }
    onSaved(result.value);
  };

  return (
    <form className="settings-dialog__form" noValidate onSubmit={(event) => void submit(event)}>
      {failure ? (
        <Alert intent="danger">
          <AlertTitle>The settings weren't saved</AlertTitle>
          <AlertDescription>{failure}</AlertDescription>
        </Alert>
      ) : null}

      <Field>
        {/* A group isn't something a <label> can point at; the field names it. */}
        <FieldLabel asChild>
          <span>Theme</span>
        </FieldLabel>
        <FieldControl>
          <RadioGroup
            className="settings-dialog__themes"
            orientation="horizontal"
            value={theme}
            onValueChange={(value) => setTheme(value as ThemeDto)}
          >
            {THEMES.map((option) => (
              <div key={option.value} className="settings-dialog__theme">
                <RadioGroupItem value={option.value} id={`settings-theme-${option.value}`} />
                <label htmlFor={`settings-theme-${option.value}`}>{option.label}</label>
              </div>
            ))}
          </RadioGroup>
        </FieldControl>
      </Field>

      <Field controlId="settings-read-only">
        <FieldLabel>Open read-only above, in megabytes</FieldLabel>
        <FieldControl>
          <NumberField
            className="settings-dialog__size"
            value={readOnlyMb}
            onValueChange={setReadOnlyMb}
            min={MIN_MB}
            max={MAX_MB}
            step={0.5}
            format={{ maximumFractionDigits: 2 }}
          />
        </FieldControl>
        <FieldDescription>
          Larger files open, but can't be changed. From {MIN_MB} to {MAX_MB} MB.
        </FieldDescription>
        <FieldError>{errors.readOnly}</FieldError>
      </Field>

      <Field controlId="settings-refuse">
        <FieldLabel>Don't open above, in megabytes</FieldLabel>
        <FieldControl>
          <NumberField
            className="settings-dialog__size"
            value={refuseMb}
            onValueChange={setRefuseMb}
            min={MIN_MB}
            max={MAX_MB}
            step={1}
            format={{ maximumFractionDigits: 2 }}
          />
        </FieldControl>
        <FieldDescription>
          Larger files show a notice instead. At least the read-only size, and at most {MAX_MB} MB.
          Both sizes apply to files opened from now on.
        </FieldDescription>
        <FieldError>{errors.refuse}</FieldError>
      </Field>

      <DialogFooter>
        <DialogClose asChild>
          <Button intent="secondary">Cancel</Button>
        </DialogClose>
        <Button type="submit" disabled={saving}>
          {saving ? "Saving…" : "Save"}
        </Button>
      </DialogFooter>
    </form>
  );
}
