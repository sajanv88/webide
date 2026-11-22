import "./branch-picker.scss";
import { GitBranchIcon } from "@nuvui/icons";
import {
  Button,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandLoading,
  toast,
} from "@nuvui/react";
import type { AppCommandId, GitBranchDto } from "@repo/contracts";
import type { CommandRegistry } from "@repo/kernel";
import { useEffect, useState, useSyncExternalStore } from "react";
import type { GitStore } from "./git-store";
import { validBranchName } from "./names";

export interface BranchPickerProps {
  readonly store: GitStore;
  readonly registry: CommandRegistry<AppCommandId>;
}

type Branches =
  | { readonly status: "loading" }
  | { readonly status: "ready"; readonly list: readonly GitBranchDto[] }
  | { readonly status: "error"; readonly message: string };

/**
 * Switch Branch: a dialog listing the local branches, and offering to create
 * one with what's typed. It registers `git.switchBranch` (while there's a
 * repository).
 */
export function BranchPicker({ store, registry }: BranchPickerProps) {
  const state = useSyncExternalStore(store.subscribe, store.getState);
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [branches, setBranches] = useState<Branches>({ status: "loading" });
  const isRepo = state.view.status === "repository";

  useEffect(() => {
    if (!isRepo) return;
    return registry.register({
      id: "git.switchBranch",
      title: "Switch Branch…",
      category: "Git",
      run: () => {
        setQuery("");
        setBranches({ status: "loading" });
        setOpen(true);
        void store
          .branches()
          .then((result) =>
            setBranches(
              result.ok
                ? { status: "ready", list: result.value }
                : { status: "error", message: result.error.message },
            ),
          );
      },
    });
  }, [registry, store, isRepo]);

  const switchTo = async (name: string, create: boolean) => {
    setOpen(false);
    const result = await store.switchBranch(name, create);
    if (result.ok) toast(create ? `Created and switched to ${name}` : `Switched to ${name}`);
    else
      toast(`Couldn't switch to ${name}`, { intent: "danger", description: result.error.message });
  };

  const typed = query.trim();
  const list = branches.status === "ready" ? branches.list : [];
  const shown = list.filter((branch) => branch.name.toLowerCase().includes(typed.toLowerCase()));
  const canCreate =
    typed !== "" && validBranchName(typed) && !list.some((branch) => branch.name === typed);

  return (
    <CommandDialog
      open={open}
      onOpenChange={setOpen}
      title="Switch branch"
      label="Branch name"
      shouldFilter={false}
      loop
    >
      <CommandInput
        placeholder="A branch to switch to, or a name for a new one"
        value={query}
        onValueChange={setQuery}
      />
      <CommandList label="Branches">
        {branches.status === "loading" ? <CommandLoading label="Reading the branches" /> : null}
        {branches.status === "error" ? (
          <CommandEmpty>Couldn't read the branches: {branches.message}</CommandEmpty>
        ) : null}
        {branches.status === "ready" ? (
          <CommandEmpty>
            {typed && !validBranchName(typed)
              ? `“${typed}” can't be a branch name.`
              : "No branch matches."}
          </CommandEmpty>
        ) : null}
        {canCreate ? (
          <CommandGroup heading="New">
            <CommandItem value={`create:${typed}`} onSelect={() => void switchTo(typed, true)}>
              Create branch “{typed}” and switch to it
            </CommandItem>
          </CommandGroup>
        ) : null}
        {shown.length > 0 ? (
          <CommandGroup heading="Branches">
            {shown.map((branch) => (
              <CommandItem
                key={branch.name}
                value={`branch:${branch.name}`}
                disabled={branch.current}
                onSelect={() => void switchTo(branch.name, false)}
              >
                {branch.name}
                {branch.current ? <span className="branch-picker__current">current</span> : null}
              </CommandItem>
            ))}
          </CommandGroup>
        ) : null}
      </CommandList>
    </CommandDialog>
  );
}

/** The branch in the status bar; pressing it switches branch. */
export function BranchStatus({
  store,
  registry,
}: {
  readonly store: GitStore;
  readonly registry: CommandRegistry<AppCommandId>;
}) {
  const state = useSyncExternalStore(store.subscribe, store.getState);
  if (state.view.status !== "repository") return null;
  const { head } = state.view;
  const name = head.branch ?? (head.commit ? `detached at ${head.commit}` : "no commit");
  const sync = [head.ahead > 0 ? `${head.ahead}↑` : "", head.behind > 0 ? `${head.behind}↓` : ""]
    .filter(Boolean)
    .join(" ");
  const said = [
    head.ahead > 0 ? `${head.ahead} to push` : "",
    head.behind > 0 ? `${head.behind} to pull` : "",
  ]
    .filter(Boolean)
    .join(", ");
  return (
    <Button
      intent="ghost"
      size="sm"
      className="branch-status"
      aria-label={`Branch ${name}${said ? `, ${said}` : ""}. Switch branch`}
      onClick={() => void registry.execute("git.switchBranch")}
    >
      <GitBranchIcon size={14} />
      {name}
      {sync ? <span className="branch-status__sync">{sync}</span> : null}
    </Button>
  );
}
