import type { AppCommandId, IpcResult, TrustPort, WorkspaceDto } from "@repo/contracts";
import { createCommandRegistry } from "@repo/kernel";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { RestrictedStatusItem } from "./restricted-status";
import { parentOf, WorkspaceTrust } from "./workspace-trust";

const folder = (trusted: boolean, id = "1"): WorkspaceDto => ({
  id,
  name: "app",
  path: "C:\\src\\app",
  trusted,
});

function port(answer: (trusted: boolean) => IpcResult<WorkspaceDto>) {
  const setTrust = vi.fn(async (trusted: boolean, _parent: boolean) => answer(trusted));
  return { setTrust } satisfies TrustPort;
}

describe("WorkspaceTrust", () => {
  it("asks when an untrusted folder opens, and trusts it", async () => {
    const registry = createCommandRegistry<AppCommandId>();
    const trust = port((trusted) => ({ ok: true, value: folder(trusted) }));
    const onChange = vi.fn();
    render(
      <WorkspaceTrust
        workspace={folder(false)}
        trusted={false}
        port={trust}
        registry={registry}
        onChange={onChange}
      />,
    );
    expect(
      screen.getByRole("dialog", { name: "Do you trust the authors of the files in this folder?" }),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("checkbox", { name: /Trust every folder in C:\\src/ }));
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Trust Folder" }));
    });
    expect(trust.setTrust).toHaveBeenCalledWith(true, true);
    expect(onChange).toHaveBeenCalledWith(true);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("doesn't ask for a trusted folder, and manages trust on request", async () => {
    const registry = createCommandRegistry<AppCommandId>();
    const trust = port((trusted) => ({ ok: true, value: folder(trusted) }));
    const onChange = vi.fn();
    render(
      <WorkspaceTrust
        workspace={folder(true)}
        trusted
        port={trust}
        registry={registry}
        onChange={onChange}
      />,
    );
    expect(screen.queryByRole("dialog")).toBeNull();
    await act(async () => {
      await registry.execute("workspace.manageTrust");
    });
    expect(screen.getByRole("dialog", { name: "You trust this folder's authors" })).toBeTruthy();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Don't Trust" }));
    });
    expect(trust.setTrust).toHaveBeenCalledWith(false, false);
    expect(onChange).toHaveBeenCalledWith(false);
  });

  it("stays restricted when closed, and says why trusting failed", async () => {
    const registry = createCommandRegistry<AppCommandId>();
    const trust = port(() => ({
      ok: false,
      error: { code: "permissionDenied", message: "can't write the trust file" },
    }));
    const onChange = vi.fn();
    render(
      <WorkspaceTrust
        workspace={folder(false)}
        trusted={false}
        port={trust}
        registry={registry}
        onChange={onChange}
      />,
    );
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Trust Folder" }));
    });
    expect(screen.getByText("can't write the trust file")).toBeTruthy();
    expect(onChange).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Stay in Restricted Mode" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(trust.setTrust).toHaveBeenCalledOnce();
  });
});

describe("RestrictedStatusItem", () => {
  it("shows while restricted and opens the dialog", () => {
    const registry = createCommandRegistry<AppCommandId>();
    const run = vi.fn();
    registry.register({ id: "workspace.manageTrust", title: "Manage", run });
    const { rerender } = render(<RestrictedStatusItem trusted={false} registry={registry} />);
    fireEvent.click(screen.getByRole("button", { name: /Restricted Mode/ }));
    expect(run).toHaveBeenCalledOnce();
    rerender(<RestrictedStatusItem trusted registry={registry} />);
    expect(screen.queryByRole("button")).toBeNull();
  });
});

describe("parentOf", () => {
  it("is the folder a path is in", () => {
    expect(parentOf("C:\\src\\app")).toBe("C:\\src");
    expect(parentOf("C:\\app")).toBe("C:\\");
    expect(parentOf("/home/me/app/")).toBe("/home/me");
    expect(parentOf("/app")).toBeNull();
  });
});
