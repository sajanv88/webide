import "./quick-open.scss";
import {
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandLoading,
} from "@nuvui/react";
import type { AppCommandId, AppEvents, SearchPort } from "@repo/contracts";
import type { CommandRegistry, EventBus } from "@repo/kernel";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { matchFiles } from "./fuzzy";
import { Highlighted, positionsToRanges, type Range } from "./highlight";

/** How many matches the list shows. */
export const QUICK_OPEN_LIMIT = 50;

export interface QuickOpenProps {
  /** The open workspace's files. */
  readonly port: SearchPort;
  readonly registry: CommandRegistry<AppCommandId>;
  readonly bus: EventBus<AppEvents>;
}

type Files =
  | { readonly status: "idle" | "loading" }
  | { readonly status: "ready"; readonly files: readonly string[]; readonly truncated: boolean }
  | { readonly status: "error"; readonly message: string };

/**
 * Go to File: a dialog listing the workspace's files, matched loosely against
 * what's typed ("dtabs" finds document-tabs.tsx). It registers
 * `search.goToFile`. The list is read when the dialog first opens and again
 * after files are added, removed or renamed.
 */
export function QuickOpen({ port, registry, bus }: QuickOpenProps) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [files, setFiles] = useState<Files>({ status: "idle" });
  const request = useRef(0);
  const openRef = useRef(false);
  openRef.current = open;

  const load = useCallback(async () => {
    const id = ++request.current;
    setFiles({ status: "loading" });
    const result = await port.listFiles();
    if (id !== request.current) return;
    setFiles(
      result.ok
        ? { status: "ready", files: result.value.files, truncated: result.value.truncated }
        : { status: "error", message: result.error.message },
    );
  }, [port]);

  useEffect(
    () =>
      registry.register({
        id: "search.goToFile",
        title: "Go to File…",
        category: "Go",
        run: () => {
          setQuery("");
          // A list that couldn't be read is tried again.
          setFiles((now) => (now.status === "error" ? { status: "idle" } : now));
          setOpen(true);
        },
      }),
    [registry],
  );

  // Read the list when the dialog opens without one.
  useEffect(() => {
    if (open && files.status === "idle") void load();
  }, [open, files.status, load]);

  // Files added, removed or renamed: the list is stale. Edits don't change it.
  useEffect(
    () =>
      bus.on("workspace.filesChanged", ({ changes, rescan }) => {
        if (!rescan && changes.every((change) => change.kind === "modified")) return;
        request.current += 1;
        if (openRef.current) void load();
        else setFiles({ status: "idle" });
      }),
    [bus, load],
  );

  const matches = useMemo(
    () => (files.status === "ready" ? matchFiles(files.files, query, QUICK_OPEN_LIMIT) : []),
    [files, query],
  );

  const choose = (path: string) => {
    setOpen(false);
    bus.emit("file.openRequested", { path });
  };

  const heading =
    files.status === "ready" && files.truncated
      ? `Files (only the first ${files.files.length.toLocaleString("en")} are listed)`
      : "Files";

  return (
    <CommandDialog
      open={open}
      onOpenChange={setOpen}
      title="Go to file"
      label="File name"
      shouldFilter={false}
      loop
    >
      <CommandInput
        placeholder="Type part of a file's name"
        value={query}
        onValueChange={setQuery}
      />
      <CommandList label="Files">
        {files.status === "loading" ? <CommandLoading label="Listing files" /> : null}
        {files.status === "error" ? (
          <CommandEmpty>Couldn't list the files: {files.message}</CommandEmpty>
        ) : null}
        {files.status === "ready" ? <CommandEmpty>No file matches.</CommandEmpty> : null}
        {matches.length > 0 ? (
          <CommandGroup heading={heading}>
            {matches.map((match) => (
              <CommandItem key={match.path} value={match.path} onSelect={() => choose(match.path)}>
                <FileLabel path={match.path} positions={match.positions} />
              </CommandItem>
            ))}
          </CommandGroup>
        ) : null}
      </CommandList>
    </CommandDialog>
  );
}

/** The file's name (with what matched marked), then its folder, quieter. */
function FileLabel({
  path,
  positions,
}: {
  readonly path: string;
  readonly positions: readonly number[];
}) {
  const nameStart = path.lastIndexOf("/") + 1;
  const ranges = positionsToRanges(positions);
  const shift = (from: number, to: number): Range[] =>
    ranges
      .filter(([start, end]) => end > from && start < to)
      .map(([start, end]) => [Math.max(start, from) - from, Math.min(end, to) - from]);
  const folder = path.slice(0, Math.max(0, nameStart - 1));
  return (
    <span className="quick-open-file">
      <span className="quick-open-file__name">
        <Highlighted text={path.slice(nameStart)} ranges={shift(nameStart, path.length)} />
      </span>
      {folder ? (
        <span className="quick-open-file__folder">
          <Highlighted text={folder} ranges={shift(0, folder.length)} />
        </span>
      ) : null}
    </span>
  );
}
