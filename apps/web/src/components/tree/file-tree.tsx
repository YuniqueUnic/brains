import {
  ChevronRight,
  Copy,
  CopyPlus,
  CornerDownRight,
  FilePlus2,
  FileText,
  Folder,
  FolderOpen,
  FolderPlus,
  Info,
  Link2,
  ListChecks,
  MoreHorizontal,
  Trash2,
  X,
} from "lucide-react";
import { ContextMenu } from "@base-ui-components/react/context-menu";
import { Menu } from "@base-ui-components/react/menu";
import { Tree, TreeItem, TreeItemContent } from "react-aria-components";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Entry } from "../../api/client";
import { cn } from "../../lib/utils";
import { menuItemClass, menuPopupClass, MenuShortcut } from "../ui/menu";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import {
  checkState,
  collapseToRoots,
  invertSelection,
  selectAllFiles,
  togglePath,
} from "./selection";

/*
 * File tree (DESIGN.md §9-15, §57): React Aria owns keyboard navigation,
 * selection (⌘/Shift) and expansion; this layer owns visuals and actions.
 * Click opens, ⌘/Shift-click selects, right-click or the hover ⋯ opens the
 * context menu, F2 renames inline, drag onto a folder moves. The toolbar
 * carries creation and an explicit batch-selection mode.
 */

export type FileNode =
  | { type: "folder"; name: string; path: string; children: FileNode[] }
  | { type: "file"; entry: Entry };

export function buildTree(files: Entry[]): FileNode[] {
  const root: Extract<FileNode, { type: "folder" }> = {
    type: "folder",
    name: "",
    path: "",
    children: [],
  };
  const folders = new Map<string, Extract<FileNode, { type: "folder" }>>([["", root]]);
  const folderOf = (dir: string) => {
    const found = folders.get(dir);
    if (found) return found;
    const segments = dir.split("/");
    const name = segments[segments.length - 1];
    const parentDir = segments.slice(0, -1).join("/");
    const parent = folderOf(parentDir);
    const node: Extract<FileNode, { type: "folder" }> = {
      type: "folder",
      name,
      path: dir,
      children: [],
    };
    folders.set(dir, node);
    parent.children.push(node);
    return node;
  };
  for (const file of files) {
    const segments = file.path.split("/");
    const dir = segments.slice(0, -1).join("/");
    folderOf(dir).children.push({ type: "file", entry: file });
  }
  const sortNodes = (nodes: FileNode[]) => {
    nodes.sort((a, b) => {
      if (a.type !== b.type) return a.type === "folder" ? -1 : 1;
      return (a.type === "folder" ? a.name : a.entry.path).localeCompare(
        b.type === "folder" ? b.name : b.entry.path,
      );
    });
    for (const node of nodes) {
      if (node.type === "folder") sortNodes(node.children);
    }
  };
  sortNodes(root.children);
  return root.children;
}

export interface TreeActions {
  onOpen: (path: string) => void;
  onStartRename: (path: string) => void;
  onCommitRename: (from: string, to: string) => void;
  onCancelRename: () => void;
  onTrash: (paths: string[]) => void;
  onDuplicate: (path: string) => void;
  onCopyPath: (path: string) => void;
  /** Copy a fetchable absolute URL for the file content (auth via token). */
  onCopyNetPath?: (path: string) => void;
  onNewDocumentIn: (folder: string) => void;
  onCreateFolderIn: (folder: string) => void;
  onProperties: (path: string) => void;
  onMove: (paths: string[]) => void;
  /** Drop move: entries (folders carry a trailing slash) into a folder. */
  onDropMove: (entries: { path: string; isFolder: boolean }[], into: string) => void;
}

/** Collect every folder prefix (root included) from a flat entry list. */
export function collectFolders(files: Entry[]): string[] {
  const folders = new Set<string>([""]);
  for (const file of files) {
    const segments = file.path.split("/");
    for (let i = 1; i < segments.length; i += 1) {
      folders.add(segments.slice(0, i).join("/"));
    }
  }
  return [...folders];
}

export function FileTree({
  files,
  selected,
  activePath,
  renamingPath,
  selecting,
  onToggleSelecting,
  actions,
  onSelectionChange,
}: {
  files: Entry[];
  selected: Set<string>;
  activePath: string | null;
  renamingPath: string | null;
  selecting: boolean;
  onToggleSelecting: () => void;
  actions: TreeActions;
  onSelectionChange: (next: Set<string>) => void;
}) {
  const { t } = useTranslation("workspace");
  const nodes = buildTree(files);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [dropTarget, setDropTarget] = useState<string | null>(null);
  const allPaths = files.map((file) => file.path);

  // F2 renames the active (or only) selected row.
  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if (event.key === "F2" && !renamingPath) {
        const target = activePath ?? (selected.size === 1 ? [...selected][0] : null);
        if (target) {
          event.preventDefault();
          actions.onStartRename(target);
        }
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [activePath, selected, renamingPath, actions]);

  const toggleFolder = (path: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };

  // Batch selection (tree/selection.ts): the set holds file paths only; a
  // folder row derives its all/partial/none state from the files beneath it,
  // so nested folders light up without folder rows ever being stored.
  const toggleSelect = (path: string, isFolder: boolean) => {
    onSelectionChange(togglePath(allPaths, path, isFolder, selected));
  };

  // Plain mouse clicks land on our row div (Base UI's context-menu trigger
  // wraps it and swallows RAC's synthetic press), so open/toggle is handled
  // here; modified clicks fall through to React Aria's selection. Keyboard
  // Enter/Space keeps flowing through TreeItem onAction untouched.
  const rowClick = (path: string, isFolder: boolean) => (event: React.MouseEvent) => {
    if (selecting) {
      event.preventDefault();
      toggleSelect(path, isFolder);
      return;
    }
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    if (isFolder) toggleFolder(path);
    else if (renamingPath !== path) actions.onOpen(path);
  };

  // Drag & drop move (DESIGN.md §58): drag a file or folder onto a folder.
  // The payload is the row plus the whole live selection when it is member,
  // collapsed to top-level entries so a batch-selected folder rides once.
  const dragPaths = (path: string, isFolder: boolean): { path: string; isFolder: boolean }[] => {
    const entry: { path: string; isFolder: boolean } = {
      path: isFolder ? `${path}/` : path,
      isFolder,
    };
    if (selected.has(path) && selected.size > 1) {
      const others = [...selected].filter((one) => one !== path);
      const folderEntries = others
        .filter((one) => !allPaths.includes(one))
        .map((one) => ({ path: one, isFolder: true }));
      const fileEntries = others
        .filter((one) => allPaths.includes(one))
        .map((one) => ({ path: one, isFolder: false }));
      return collapseToRoots([entry, ...fileEntries, ...folderEntries]);
    }
    if (isFolder) return collapseToRoots([entry]);
    return [entry];
  };

  const handleDrop = (into: string) => (event: React.DragEvent) => {
    event.preventDefault();
    event.stopPropagation();
    setDropTarget(null);
    const raw = event.dataTransfer.getData("application/x-brains-paths");
    if (!raw) return;
    try {
      const entries = JSON.parse(raw) as { path: string; isFolder: boolean }[];
      const movable = entries.filter((one) => one.path !== `${into}/`);
      if (movable.length > 0) actions.onDropMove(movable, into);
    } catch {
      /* foreign drag payload — ignore */
    }
  };

  const renderNodes = (nodes: FileNode[]) =>
    nodes.map((node) => {
      if (node.type === "folder") {
        const isExpanded = !collapsed.has(node.path);
        const folderState = checkState(allPaths, node.path, true, selected);
        return (
          <TreeItem
            key={node.path}
            id={node.path}
            textValue={node.name}
            onAction={() => {
              if (selecting) toggleSelect(node.path, true);
              else toggleFolder(node.path);
            }}
          >
            <TreeItemContent>
              {(props) => (
                <ContextMenu.Root>
                  <ContextMenu.Trigger
                    render={
                      <div
                        draggable={!selecting}
                        onDragStart={(event) => {
                          event.dataTransfer.effectAllowed = "move";
                          event.dataTransfer.setData(
                            "application/x-brains-paths",
                            JSON.stringify(dragPaths(node.path, true)),
                          );
                        }}
                        onClick={rowClick(node.path, true)}
                        onDragOver={(event) => {
                          event.preventDefault();
                          event.stopPropagation();
                          setDropTarget(node.path);
                        }}
                        onDragLeave={() => setDropTarget(null)}
                        onDrop={handleDrop(node.path)}
                        className={cn(
                          "group/row flex h-7 select-none items-center gap-1.5 rounded-md pr-1 text-body transition-colors",
                          selecting && folderState !== "none" && "bg-selected",
                          dropTarget === node.path && "ring-2 ring-inset ring-primary/50",
                          !selecting && "cursor-pointer hover:bg-muted",
                        )}
                        style={{ paddingLeft: rowIndent(props.level) }}
                      />
                    }
                  >
                    <FolderChevron
                      isExpanded={props.isExpanded}
                      onToggle={() => toggleFolder(node.path)}
                    />
                    {props.isExpanded ? (
                      <FolderOpen className="size-4 shrink-0 stroke-[1.5] text-muted-foreground" />
                    ) : (
                      <Folder className="size-4 shrink-0 stroke-[1.5] text-muted-foreground" />
                    )}
                    <span className="min-w-0 flex-1 truncate">{node.name}</span>
                    {selecting && (
                      <RowCheckbox
                        label={t("selectOne", { path: node.path })}
                        state={folderState}
                        onToggle={() => toggleSelect(node.path, true)}
                      />
                    )}
                    <RowMenu actions={actions} path={node.path} isFolder />
                  </ContextMenu.Trigger>
                  <Menu.Portal>
                    <Menu.Positioner sideOffset={2} align="start">
                      <Menu.Popup className={menuPopupClass}>
                        <FolderMenuItems actions={actions} path={node.path} />
                      </Menu.Popup>
                    </Menu.Positioner>
                  </Menu.Portal>
                </ContextMenu.Root>
              )}
            </TreeItemContent>
            {isExpanded && renderNodes(node.children)}
          </TreeItem>
        );
      }
      const entry = node.entry;
      const isRenaming = renamingPath === entry.path;
      return (
        <TreeItem
          key={entry.path}
          id={entry.path}
          textValue={entry.path}
          onAction={() => {
            if (selecting) toggleSelect(entry.path, false);
            else if (!isRenaming) actions.onOpen(entry.path);
          }}
        >
          <TreeItemContent>
            {(props) => (
              <ContextMenu.Root>
                <ContextMenu.Trigger
                  render={
                    <div
                      draggable={!selecting && !isRenaming}
                      onDragStart={(event) => {
                        event.dataTransfer.effectAllowed = "move";
                        event.dataTransfer.setData(
                          "application/x-brains-paths",
                          JSON.stringify(dragPaths(entry.path, false)),
                        );
                      }}
                      onClick={rowClick(entry.path, false)}
                      className={cn(
                        "group/row flex h-7 select-none items-center gap-1.5 rounded-md pr-1 text-body outline-none transition-colors",
                        props.isSelected || activePath === entry.path
                          ? "bg-selected text-foreground"
                          : "hover:bg-muted",
                        !selecting && "cursor-pointer",
                      )}
                      style={{ paddingLeft: fileIndent(props.level) }}
                      title={entry.path}
                    />
                  }
                >
                  <FileText className="size-4 shrink-0 stroke-[1.5] text-faint" />
                  {isRenaming ? (
                    <InlineRename
                      initial={entry.path}
                      onCommit={(to) => actions.onCommitRename(entry.path, to)}
                      onCancel={actions.onCancelRename}
                    />
                  ) : (
                    <span className="file-path min-w-0 flex-1 truncate">{entry.path}</span>
                  )}
                  {selecting && (
                    <RowCheckbox
                      label={t("selectOne", { path: entry.path })}
                      state={selected.has(entry.path) ? "all" : "none"}
                      onToggle={() => toggleSelect(entry.path, false)}
                    />
                  )}
                  <RowMenu actions={actions} path={entry.path} />
                </ContextMenu.Trigger>
                <Menu.Portal>
                  <Menu.Positioner sideOffset={2} align="start">
                    <Menu.Popup className={menuPopupClass}>
                      <FileMenuItems
                        actions={actions}
                        path={entry.path}
                        renaming={isRenaming}
                      />
                    </Menu.Popup>
                  </Menu.Positioner>
                </Menu.Portal>
              </ContextMenu.Root>
            )}
          </TreeItemContent>
        </TreeItem>
      );
    });

  return (
    <div
      className="flex min-h-0 flex-1 flex-col"
      onDrop={handleDrop("")}
      onDragOver={(event) => {
        if (event.dataTransfer.types.includes("application/x-brains-paths")) {
          event.preventDefault();
        }
      }}
    >
      {/* Toolbar (creation + batch selection) */}
      <div className="flex h-8 shrink-0 items-center gap-0.5 px-2">
        <Menu.Root>
          <Menu.Trigger
            render={<Button size="iconSm" aria-label={t("newDocument")} data-testid="tree-new" />}
          >
            <FilePlus2 />
          </Menu.Trigger>
          <Menu.Portal>
            <Menu.Positioner sideOffset={4} align="start">
              <Menu.Popup className={menuPopupClass}>
                <Menu.Item
                  className={menuItemClass()}
                  onClick={() => actions.onNewDocumentIn("")}
                  data-testid="tree-new-document"
                >
                  <FilePlus2 className="size-4 stroke-[1.5] text-muted-foreground" />
                  {t("newDocument")}
                </Menu.Item>
                <Menu.Item
                  className={menuItemClass()}
                  onClick={() => actions.onCreateFolderIn("")}
                  data-testid="tree-new-folder"
                >
                  <FolderPlus className="size-4 stroke-[1.5] text-muted-foreground" />
                  {t("newFolder")}
                </Menu.Item>
              </Menu.Popup>
            </Menu.Positioner>
          </Menu.Portal>
        </Menu.Root>
        <span className="flex-1" />
        <Button
          variant={selecting ? "secondary" : "ghost"}
          size="iconSm"
          aria-pressed={selecting}
          aria-label={selecting ? t("exitSelect") : t("select")}
          data-testid="toggle-select"
          onClick={onToggleSelecting}
        >
          {selecting ? <X /> : <ListChecks />}
        </Button>
      </div>

      {selecting && (
        <div className="mx-2 mb-1 flex shrink-0 items-center gap-1 rounded-md bg-muted/60 px-2 py-1 text-meta text-muted-foreground">
          <span className="min-w-0 flex-1 truncate">
            {t("selectedCount", { count: selected.size })}
          </span>
          <button
            type="button"
            className="cursor-pointer rounded px-1.5 py-0.5 hover:bg-muted hover:text-foreground"
            onClick={() => onSelectionChange(selectAllFiles(allPaths))}
          >
            {t("selectAll")}
          </button>
          <button
            type="button"
            className="cursor-pointer rounded px-1.5 py-0.5 hover:bg-muted hover:text-foreground"
            onClick={() => onSelectionChange(invertSelection(allPaths, selected))}
          >
            {t("invertSelection")}
          </button>
          <button
            type="button"
            data-testid="delete-selected"
            disabled={selected.size === 0}
            className="cursor-pointer rounded px-1.5 py-0.5 text-destructive hover:bg-destructive/10 disabled:pointer-events-none disabled:opacity-45"
            onClick={() => actions.onTrash([...selected])}
          >
            {t("deleteSelected")}
          </button>
        </div>
      )}

      <div className="min-h-0 flex-1 overflow-auto px-1 pb-1" data-testid="file-list">
        <Tree
          aria-label={t("panelFiles")}
          selectionMode="multiple"
          selectionBehavior="replace"
          selectedKeys={selected}
          onSelectionChange={(keys) => {
            if (selecting) return;
            const next = new Set<string>();
            for (const key of keys) next.add(String(key));
            onSelectionChange(next);
          }}
          expandedKeys={allExpandedKeys(nodes, collapsed)}
          onExpandedChange={(keys) => {
            const expanded = new Set(keys as Set<unknown>);
            const next = new Set<string>();
            for (const folder of collectFolders(files)) {
              if (folder && !expanded.has(folder)) next.add(folder);
            }
            setCollapsed(next);
          }}
          className="outline-none"
          renderEmptyState={() => (
            <p className="px-3 py-8 text-center text-xs text-muted-foreground">{t("noFiles")}</p>
          )}
        >
          {renderNodes(nodes)}
        </Tree>
      </div>
    </div>
  );
}

/** Root rows start at the container padding; folders keep the chevron inside. */
function rowIndent(level: number): string {
  return `${6 + (Math.max(level, 1) - 1) * 16}px`;
}
function fileIndent(level: number): string {
  // Align file names past the folder chevron of the same level.
  return `${6 + (Math.max(level, 1) - 1) * 16 + 22}px`;
}

function allExpandedKeys(nodes: FileNode[], collapsed: Set<string>): Set<string> {
  const keys = new Set<string>();
  const walk = (items: FileNode[]) => {
    for (const node of items) {
      if (node.type === "folder") {
        if (!collapsed.has(node.path)) keys.add(node.path);
        walk(node.children);
      }
    }
  };
  walk(nodes);
  return keys;
}

/** Chevron button: expands/collapses without re-triggering the row action. */
function FolderChevron({
  isExpanded,
  onToggle,
}: {
  isExpanded: boolean;
  onToggle: () => void;
}) {
  return (
    <button
      type="button"
      tabIndex={-1}
      aria-hidden
      className="flex size-4 shrink-0 cursor-pointer items-center justify-center rounded text-faint hover:text-foreground"
      onClick={(event) => {
        event.stopPropagation();
        onToggle();
      }}
    >
      <ChevronRight
        className={cn("size-3.5 stroke-[1.5] transition-transform", isExpanded && "rotate-90")}
      />
    </button>
  );
}

/** Native checkbox with the full all/partial/none tri-state (partial renders
 * as the browser's indeterminate dash; aria-checked follows). */
function RowCheckbox({
  label,
  state,
  onToggle,
}: {
  label: string;
  state: "all" | "partial" | "none";
  onToggle: () => void;
}) {
  return (
    <input
      type="checkbox"
      aria-label={label}
      aria-checked={state === "partial" ? "mixed" : state === "all"}
      className="size-3.5 shrink-0 accent-[var(--primary)]"
      checked={state === "all"}
      ref={(element) => {
        if (element) element.indeterminate = state === "partial";
      }}
      onClick={(event) => event.stopPropagation()}
      onChange={onToggle}
    />
  );
}

/**
 * Hover-revealed ⋯ trigger, right-aligned (DESIGN.md §55-①). Kept in the
 * layout with `hidden` + `group-hover/row:flex` replaced by opacity so the
 * trigger always owns a box: a display:none trigger leaves Base UI's
 * positioner without an anchor and the open menu teleports to 0,0.
 */
function RowMenu({
  actions,
  path,
  isFolder = false,
}: {
  actions: TreeActions;
  path: string;
  isFolder?: boolean;
}) {
  const { t } = useTranslation("workspace");
  return (
    <Menu.Root>
      <Menu.Trigger
        render={
          <button
            aria-label={t("rowMenu", { path })}
            className="hit pointer-events-none ml-auto size-5 shrink-0 cursor-pointer items-center justify-center rounded text-faint opacity-0 hover:bg-selected hover:text-foreground group-hover/row:pointer-events-auto group-hover/row:opacity-100 focus-visible:pointer-events-auto focus-visible:opacity-100 focus-visible:outline-2 focus-visible:outline-ring/70"
            onClick={(event) => event.stopPropagation()}
          />
        }
      >
        <MoreHorizontal className="size-3.5 stroke-[1.5]" />
      </Menu.Trigger>
      <Menu.Portal>
        <Menu.Positioner sideOffset={2} align="start">
          <Menu.Popup className={menuPopupClass}>
            {isFolder ? (
              <FolderMenuItems actions={actions} path={path} />
            ) : (
              <FileMenuItems actions={actions} path={path} renaming={false} />
            )}
          </Menu.Popup>
        </Menu.Positioner>
      </Menu.Portal>
    </Menu.Root>
  );
}

function FileMenuItems({
  actions,
  path,
  renaming,
}: {
  actions: TreeActions;
  path: string;
  renaming: boolean;
}) {
  const { t } = useTranslation("workspace");
  return (
    <>
      <Menu.Item className={menuItemClass()} onClick={() => actions.onOpen(path)}>
        {t("menuOpen")}
      </Menu.Item>
      <Menu.Item
        className={menuItemClass()}
        onClick={() => actions.onStartRename(path)}
        data-testid="menu-rename"
      >
        {t("menuRename")}
        <MenuShortcut>F2</MenuShortcut>
      </Menu.Item>
      <Menu.Item className={menuItemClass()} onClick={() => actions.onDuplicate(path)}>
        <CopyPlus className="size-4 stroke-[1.5] text-muted-foreground" />
        {t("menuDuplicate")}
      </Menu.Item>
      <Menu.Item className={menuItemClass()} onClick={() => actions.onMove([path])}>
        <CornerDownRight className="size-4 stroke-[1.5] text-muted-foreground" />
        {t("menuMove")}
      </Menu.Item>
      <Menu.Item className={menuItemClass()} onClick={() => actions.onCopyPath(path)}>
        <Copy className="size-4 stroke-[1.5] text-muted-foreground" />
        {t("menuCopyPath")}
      </Menu.Item>
      {actions.onCopyNetPath && (
        <Menu.Item
          className={menuItemClass()}
          onClick={() => actions.onCopyNetPath?.(path)}
          data-testid="menu-copy-net-path"
        >
          <Link2 className="size-4 stroke-[1.5] text-muted-foreground" />
          {t("menuCopyNetPath")}
        </Menu.Item>
      )}
      <Menu.Separator className="mx-2 my-1 h-px bg-border" />
      <Menu.Item
        className={menuItemClass()}
        onClick={() => actions.onProperties(path)}
        data-testid="menu-properties"
      >
        <Info className="size-4 stroke-[1.5] text-muted-foreground" />
        {t("menuProperties")}
        <MenuShortcut>⌘I</MenuShortcut>
      </Menu.Item>
      <Menu.Separator className="mx-2 my-1 h-px bg-border" />
      {!renaming && (
        <Menu.Item
          className={menuItemClass({ danger: true })}
          onClick={() => actions.onTrash([path])}
        >
          <Trash2 className="size-4 stroke-[1.5]" />
          {t("menuTrash")}
        </Menu.Item>
      )}
    </>
  );
}

function FolderMenuItems({ actions, path }: { actions: TreeActions; path: string }) {
  const { t } = useTranslation("workspace");
  return (
    <>
      <Menu.Item
        className={menuItemClass()}
        onClick={() => actions.onNewDocumentIn(path)}
      >
        <FilePlus2 className="size-4 stroke-[1.5] text-muted-foreground" />
        {t("menuNewDocument")}
      </Menu.Item>
      <Menu.Item
        className={menuItemClass()}
        onClick={() => actions.onCreateFolderIn(path)}
      >
        <FolderPlus className="size-4 stroke-[1.5] text-muted-foreground" />
        {t("menuNewFolder")}
      </Menu.Item>
      <Menu.Separator className="mx-2 my-1 h-px bg-border" />
      <Menu.Item className={menuItemClass()} onClick={() => actions.onMove([`${path}/`])}>
        <CornerDownRight className="size-4 stroke-[1.5] text-muted-foreground" />
        {t("menuMove")}
      </Menu.Item>
      <Menu.Item className={menuItemClass()} onClick={() => actions.onCopyPath(`${path}/`)}>
        <Copy className="size-4 stroke-[1.5] text-muted-foreground" />
        {t("menuCopyPath")}
      </Menu.Item>
      <Menu.Separator className="mx-2 my-1 h-px bg-border" />
      <Menu.Item
        className={menuItemClass({ danger: true })}
        onClick={() => actions.onTrash([`${path}/`])}
      >
        <Trash2 className="size-4 stroke-[1.5]" />
        {t("menuTrash")}
      </Menu.Item>
    </>
  );
}

/**
 * Inline rename (DESIGN.md §15): no dialog — the row becomes an input,
 * Enter commits, Esc cancels, blur cancels. The whole path is editable
 * (moving across folders is a legal rename).
 */
function InlineRename({
  initial,
  onCommit,
  onCancel,
}: {
  initial: string;
  onCommit: (to: string) => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation("workspace");
  const ref = useRef<HTMLInputElement | null>(null);
  const [value, setValue] = useState(initial);
  useEffect(() => {
    ref.current?.focus();
    ref.current?.select();
  }, []);
  const commit = () => {
    const next = value.trim();
    if (!next || next === initial) onCancel();
    else onCommit(next);
  };
  return (
    <Input
      ref={ref}
      aria-label={t("renameAria")}
      className="h-6 flex-1 px-1.5 py-0 font-mono text-xs"
      value={value}
      onChange={(event) => setValue(event.target.value)}
      onClick={(event) => event.stopPropagation()}
      onKeyDown={(event) => {
        event.stopPropagation();
        if (event.key === "Enter") commit();
        if (event.key === "Escape") onCancel();
      }}
      onBlur={commit}
    />
  );
}
