import "./explorer.scss";
import "./explorer-node.scss";
import { CollapseIcon, FolderIcon, FolderOpenIcon, LinkIcon, RefreshIcon } from "@nuvui/icons";
import {
  Button,
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
  ScrollArea,
  Tree,
  TreeItem,
} from "@nuvui/react";
import type {
  AppCommandId,
  AppEvents,
  ExplorerPort,
  FileNodeDto,
  WorkspaceDto,
} from "@repo/contracts";
import { FileTypeIcon } from "@repo/file-icons";
import type { CommandRegistry, EventBus } from "@repo/kernel";
import { type ReactNode, useEffect, useId, useMemo, useState } from "react";
import { DEFAULT_PAGE_SIZE, useDirectoryTree } from "./use-directory-tree";

/** Values of rows that aren't files. A path never contains NUL. */
const PLACEHOLDER = "\u0000";
const MORE = `${PLACEHOLDER}more:`;

export interface ExplorerSource {
  readonly workspace: WorkspaceDto;
  readonly port: ExplorerPort;
}

export interface ExplorerProps {
  /** The open workspace and how to list it. With no folder open, the app shows no explorer. */
  readonly source: ExplorerSource;
  readonly registry: CommandRegistry<AppCommandId>;
  readonly bus: EventBus<AppEvents>;
  /** CSP nonce for nuvui ScrollArea's `<style>` element. */
  readonly styleNonce?: string | undefined;
  /** Rows of one folder drawn before a "Show more" row. */
  readonly pageSize?: number;
}

/** The explorer pane: the open folder as a tree, loaded one level at a time. */
export function Explorer(props: ExplorerProps) {
  const headingId = useId();
  return (
    <section className="explorer" aria-labelledby={headingId}>
      {/* A new workspace starts from a clean tree. */}
      <WorkspaceTree key={props.source.workspace.id} headingId={headingId} {...props} />
    </section>
  );
}

function WorkspaceTree({
  source,
  headingId,
  registry,
  bus,
  styleNonce,
  pageSize = DEFAULT_PAGE_SIZE,
}: ExplorerProps & { readonly headingId: string }) {
  const tree = useDirectoryTree(source.port, pageSize);
  const [selected, setSelected] = useState<string[]>([]);
  const { refresh, collapseAll, applyChanges } = tree;

  useEffect(() => {
    const disposers = [
      registry.register({
        id: "explorer.refresh",
        title: "Refresh Explorer",
        category: "Explorer",
        run: refresh,
      }),
      registry.register({
        id: "explorer.collapseAll",
        title: "Collapse All Folders",
        category: "Explorer",
        run: collapseAll,
      }),
    ];
    return () => {
      for (const dispose of disposers) dispose();
    };
  }, [registry, refresh, collapseAll]);

  // Changes on disk, from this app or any other.
  useEffect(
    () => bus.on("workspace.filesChanged", ({ changes, rescan }) => applyChanges(changes, rescan)),
    [bus, applyChanges],
  );

  const expandedSet = useMemo(() => new Set(tree.expanded), [tree.expanded]);

  const onValueChange = (value: string[]) => {
    const chosen = value[0];
    if (chosen?.startsWith(MORE)) {
      tree.showMore(chosen.slice(MORE.length));
      return;
    }
    if (chosen?.startsWith(PLACEHOLDER)) return;
    setSelected(value);
    if (chosen && tree.nodeAt(chosen)?.kind === "file") {
      bus.emit("explorer.fileActivated", { path: chosen });
    }
  };

  const renderNode = (entry: FileNodeDto): ReactNode => {
    const isFolder = entry.kind === "directory";
    const open = isFolder && expandedSet.has(entry.path);
    return (
      <TreeItem
        key={entry.path}
        value={entry.path}
        textValue={entry.name}
        label={<NodeLabel entry={entry} open={open} />}
      >
        {isFolder
          ? open
            ? renderFolder(entry.path)
            : // Rows inside a closed row aren't drawn; this one only gives it a chevron.
              placeholder(`closed:${entry.path}`, "")
          : null}
      </TreeItem>
    );
  };

  const renderFolder = (path: string): ReactNode[] => {
    const state = tree.folders.get(path);
    const folder = state?.status === "loading" ? state.previous : state;
    if (!folder) return [placeholder(`loading:${path}`, "Loading…")];
    if (folder.status === "error") {
      return [placeholder(`error:${path}`, `Couldn't open this folder: ${folder.message}`)];
    }
    if (folder.entries.length === 0) return [placeholder(`empty:${path}`, "Empty folder")];

    const rows = folder.entries.slice(0, folder.shown).map(renderNode);
    const hidden = folder.entries.length - folder.shown;
    if (hidden > 0) {
      const label = `Show ${Math.min(pageSize, hidden)} more (${hidden} not shown)`;
      rows.push(<TreeItem key={`${MORE}${path}`} value={`${MORE}${path}`} label={label} />);
    }
    if (folder.truncated) {
      rows.push(
        placeholder(`truncated:${path}`, "This folder has more entries than can be listed"),
      );
    }
    if (folder.skipped > 0) {
      rows.push(
        placeholder(`skipped:${path}`, `${folder.skipped} entries with unreadable names not shown`),
      );
    }
    return rows;
  };

  const { workspace } = source;
  return (
    <>
      <div className="explorer__header">
        <h2 id={headingId} className="explorer__title" title={workspace.path}>
          {workspace.name}
        </h2>
        <div className="explorer__actions">
          <Button
            intent="ghost"
            size="sm"
            className="explorer__icon"
            aria-label="Refresh explorer"
            onClick={refresh}
          >
            <RefreshIcon size={16} />
          </Button>
          <Button
            intent="ghost"
            size="sm"
            className="explorer__icon"
            aria-label="Collapse all folders"
            onClick={collapseAll}
          >
            <CollapseIcon size={16} />
          </Button>
        </div>
      </div>
      <ContextMenu>
        <ContextMenuTrigger asChild>
          <div className="explorer__body">
            <ScrollArea
              className="explorer__scroll"
              aria-label={`Files in ${workspace.name}`}
              {...(styleNonce ? { nonce: styleNonce } : {})}
            >
              <Tree
                aria-label={`Files in ${workspace.name}`}
                className="explorer__tree"
                selectionMode="single"
                value={selected}
                onValueChange={onValueChange}
                expanded={[...tree.expanded]}
                onExpandedChange={tree.setExpanded}
              >
                {renderFolder("")}
              </Tree>
            </ScrollArea>
          </div>
        </ContextMenuTrigger>
        <ContextMenuContent aria-label="Explorer">
          <ContextMenuItem onSelect={refresh}>Refresh</ContextMenuItem>
          <ContextMenuItem onSelect={collapseAll}>Collapse all folders</ContextMenuItem>
        </ContextMenuContent>
      </ContextMenu>
    </>
  );
}

function placeholder(id: string, label: string): ReactNode {
  return (
    <TreeItem key={`${PLACEHOLDER}${id}`} value={`${PLACEHOLDER}${id}`} label={label} disabled />
  );
}

function NodeLabel({ entry, open }: { readonly entry: FileNodeDto; readonly open: boolean }) {
  const FolderGlyph = open ? FolderOpenIcon : FolderIcon;
  return (
    <span className="explorer-node">
      {entry.kind === "directory" ? (
        <FolderGlyph size={16} className="explorer-node__icon" />
      ) : (
        <FileTypeIcon name={entry.name} className="explorer-node__icon" />
      )}
      <span className="explorer-node__name">{entry.name}</span>
      {entry.isSymlink ? (
        <LinkIcon size={12} className="explorer-node__link" title="Symbolic link" />
      ) : null}
    </span>
  );
}
