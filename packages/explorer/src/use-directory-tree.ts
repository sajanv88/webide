import type { ExplorerPort, FileChangeDto, FileNodeDto } from "@repo/contracts";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

/** How many rows of one folder are drawn before a "Show more" row. */
export const DEFAULT_PAGE_SIZE = 500;

export type FolderState =
  | { readonly status: "loading"; readonly previous?: LoadedFolder }
  | { readonly status: "error"; readonly message: string }
  | LoadedFolder;

export interface LoadedFolder {
  readonly status: "loaded";
  readonly entries: readonly FileNodeDto[];
  /** The backend listed only part of the folder. */
  readonly truncated: boolean;
  readonly skipped: number;
  /** How many entries are drawn; more are drawn with `showMore`. */
  readonly shown: number;
}

export interface DirectoryTree {
  /** Folder states by workspace-relative path; `""` is the root. */
  readonly folders: ReadonlyMap<string, FolderState>;
  readonly expanded: readonly string[];
  readonly setExpanded: (next: string[]) => void;
  readonly nodeAt: (path: string) => FileNodeDto | undefined;
  readonly showMore: (path: string) => void;
  /** Reloads the root and every open folder; closed ones are reloaded when opened. */
  readonly refresh: () => void;
  readonly collapseAll: () => void;
  /**
   * Changes on disk: reloads only the loaded folders they touch, and forgets
   * folders that were removed. With `rescan`, the same as `refresh`.
   */
  readonly applyChanges: (changes: readonly FileChangeDto[], rescan: boolean) => void;
}

const parentOf = (path: string): string => path.slice(0, Math.max(0, path.lastIndexOf("/")));

/** `path` is `root` or inside it. */
const within = (path: string, root: string): boolean =>
  path === root || path.startsWith(`${root}/`);

/**
 * Loads folders lazily, one level at a time: whenever a folder is open (or is
 * the root) and has no state yet, it's fetched. A folder that failed is
 * fetched again the next time it's opened.
 */
export function useDirectoryTree(port: ExplorerPort, pageSize = DEFAULT_PAGE_SIZE): DirectoryTree {
  const [folders, setFolders] = useState<ReadonlyMap<string, FolderState>>(() => new Map());
  const [expanded, setExpandedState] = useState<readonly string[]>([]);
  const inFlight = useRef(new Set<string>());
  // Folders that changed while they were being listed: listed again after.
  const stale = useRef(new Set<string>());
  const foldersRef = useRef(folders);
  foldersRef.current = folders;
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const load = useCallback(
    async (path: string): Promise<void> => {
      if (inFlight.current.has(path)) {
        stale.current.add(path);
        return;
      }
      inFlight.current.add(path);
      setFolders((prev) => {
        const current = prev.get(path);
        const previous = current?.status === "loaded" ? current : undefined;
        return new Map(prev).set(
          path,
          previous ? { status: "loading", previous } : { status: "loading" },
        );
      });
      const result = await port.listDirectory(path);
      inFlight.current.delete(path);
      if (!mounted.current) return;
      setFolders((prev) => {
        const next = new Map(prev);
        const current = prev.get(path);
        const keptShown = current?.status === "loading" ? (current.previous?.shown ?? 0) : 0;
        next.set(
          path,
          result.ok
            ? {
                status: "loaded",
                entries: result.value.entries,
                truncated: result.value.truncated,
                skipped: result.value.skipped,
                shown: Math.max(keptShown, pageSize),
              }
            : { status: "error", message: result.error.message },
        );
        return next;
      });
      if (stale.current.delete(path)) await load(path);
    },
    [port, pageSize],
  );

  // Every open folder (and the root) must be loaded.
  useEffect(() => {
    for (const path of ["", ...expanded]) {
      if (!folders.has(path)) void load(path);
    }
  }, [expanded, folders, load]);

  const setExpanded = useCallback(
    (next: string[]) => {
      const closed = expanded.filter((path) => !next.includes(path));
      if (closed.some((path) => folders.get(path)?.status === "error")) {
        // Forget failures, so opening the folder again retries.
        setFolders((prev) => {
          const map = new Map(prev);
          for (const path of closed) if (map.get(path)?.status === "error") map.delete(path);
          return map;
        });
      }
      setExpandedState(next);
    },
    [expanded, folders],
  );

  const nodes = useMemo(() => {
    const map = new Map<string, FileNodeDto>();
    for (const folder of folders.values()) {
      const loaded = folder.status === "loading" ? folder.previous : folder;
      if (loaded?.status !== "loaded") continue;
      for (const entry of loaded.entries) map.set(entry.path, entry);
    }
    return map;
  }, [folders]);

  const nodeAt = useCallback((path: string) => nodes.get(path), [nodes]);

  const showMore = useCallback(
    (path: string) => {
      setFolders((prev) => {
        const folder = prev.get(path);
        if (folder?.status !== "loaded") return prev;
        return new Map(prev).set(path, { ...folder, shown: folder.shown + pageSize });
      });
    },
    [pageSize],
  );

  const refresh = useCallback(() => {
    const open = new Set(["", ...expanded]);
    // Closed folders are dropped from the cache; they reload when opened.
    setFolders((prev) => new Map([...prev].filter(([path]) => open.has(path))));
    for (const path of open) void load(path);
  }, [expanded, load]);

  const collapseAll = useCallback(() => setExpandedState([]), []);

  const applyChanges = useCallback(
    (changes: readonly FileChangeDto[], rescan: boolean) => {
      if (rescan) {
        refresh();
        return;
      }
      const touched = new Set<string>();
      const gone: string[] = [];
      for (const change of changes) {
        touched.add(parentOf(change.path));
        if (change.kind === "removed") gone.push(change.path);
        if (change.kind === "renamed") {
          touched.add(parentOf(change.from));
          gone.push(change.from);
        }
      }
      const isGone = (path: string) => gone.some((root) => within(path, root));
      if (gone.length > 0) {
        // A removed folder's listings, and its open state, go with it.
        setFolders((prev) => {
          const kept = [...prev].filter(([path]) => path === "" || !isGone(path));
          return kept.length === prev.size ? prev : new Map(kept);
        });
        setExpandedState((prev) => (prev.some(isGone) ? prev.filter((p) => !isGone(p)) : prev));
      }
      // Only folders already listed are listed again; the rest load when opened.
      for (const path of touched) {
        if (foldersRef.current.has(path) && (path === "" || !isGone(path))) void load(path);
      }
    },
    [refresh, load],
  );

  return { folders, expanded, setExpanded, nodeAt, showMore, refresh, collapseAll, applyChanges };
}
