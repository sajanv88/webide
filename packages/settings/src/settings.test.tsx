import type {
  AppCommandId,
  AppEvents,
  IpcResult,
  SettingsDto,
  SettingsPatchDto,
  SettingsPort,
} from "@repo/contracts";
import { createCommandRegistry, createEventBus } from "@repo/kernel";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { SettingsDialog } from "./settings-dialog";

const MIB = 1024 * 1024;
const initial: SettingsDto = {
  theme: "system",
  readOnlyAboveBytes: MIB,
  refuseAboveBytes: 20 * MIB,
};

function setup(update?: (patch: SettingsPatchDto) => IpcResult<SettingsDto>) {
  const patches: SettingsPatchDto[] = [];
  const port: SettingsPort = {
    get: async () => ({ ok: true, value: initial }),
    update: async (patch) => {
      patches.push(patch);
      return update?.(patch) ?? { ok: true, value: { ...initial, ...patch } as SettingsDto };
    },
  };
  const registry = createCommandRegistry<AppCommandId>();
  const bus = createEventBus<AppEvents>();
  const changed: SettingsDto[] = [];
  bus.on("settings.changed", (settings) => changed.push(settings));
  render(<SettingsDialog port={port} registry={registry} bus={bus} />);
  const open = async () => {
    await act(async () => {
      await registry.execute("settings.open");
    });
    return screen.findByRole("dialog", { name: "Settings" });
  };
  return { registry, patches, changed, open };
}

/** Types into a number field and leaves it, which is when it takes the number. */
function enter(field: HTMLElement, value: string) {
  fireEvent.focus(field);
  fireEvent.change(field, { target: { value } });
  fireEvent.blur(field);
}

describe("SettingsDialog", () => {
  it("registers settings.open, and shows the current settings with named fields", async () => {
    const { registry, open } = setup();
    expect(registry.get("settings.open")).toMatchObject({
      title: "Settings…",
      category: "Preferences",
    });
    const dialog = await open();
    const themes = await within(dialog).findByRole("radiogroup", { name: "Theme" });
    expect(
      within(themes)
        .getByRole("radio", { name: "Same as the system" })
        .getAttribute("aria-checked"),
    ).toBe("true");
    const readOnly = within(dialog).getByRole("spinbutton", {
      name: "Open read-only above, in megabytes",
    }) as HTMLInputElement;
    expect(readOnly.getAttribute("aria-valuenow")).toBe("1");
    expect(
      within(dialog)
        .getByRole("spinbutton", { name: "Don't open above, in megabytes" })
        .getAttribute("aria-valuenow"),
    ).toBe("20");
  });

  it("saves the theme and the sizes in bytes, announces them, and closes", async () => {
    const { open, patches, changed } = setup();
    const dialog = await open();
    fireEvent.click(await within(dialog).findByRole("radio", { name: "Dark" }));
    enter(
      within(dialog).getByRole("spinbutton", { name: "Open read-only above, in megabytes" }),
      "2.5",
    );
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));

    await waitFor(() => expect(patches).toHaveLength(1));
    expect(patches[0]).toEqual({
      theme: "dark",
      readOnlyAboveBytes: 2.5 * MIB,
      refuseAboveBytes: 20 * MIB,
    });
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(changed).toEqual([
      { theme: "dark", readOnlyAboveBytes: 2.5 * MIB, refuseAboveBytes: 20 * MIB },
    ]);
  });

  it("won't save a read-only size above the don't-open size, and says why on the field", async () => {
    const { open, patches } = setup();
    const dialog = await open();
    const refuse = await within(dialog).findByRole("spinbutton", {
      name: "Don't open above, in megabytes",
    });
    enter(refuse, "0.5");
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(refuse.getAttribute("aria-invalid")).toBe("true"));
    expect(within(dialog).getByText("Enter at least 1 MB, the read-only size above.")).toBeTruthy();
    expect(document.activeElement).toBe(refuse);
    expect(patches).toEqual([]);
  });

  it("keeps the dialog open with the backend's reason when saving fails", async () => {
    const { open, changed } = setup(() => ({
      ok: false,
      error: { code: "io", message: "The disk is full." },
    }));
    const dialog = await open();
    await within(dialog).findByRole("radiogroup", { name: "Theme" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    const alert = await within(dialog).findByRole("alert");
    expect(alert.textContent).toContain("The disk is full.");
    expect(changed).toEqual([]);
  });
});
