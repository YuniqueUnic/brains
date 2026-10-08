import {
  ArchiveRestore,
  Brain,
  ChevronRight,
  FileText,
  Folder,
  FolderOpen,
  Languages,
  ListChecks,
  LoaderCircle,
  LogOut,
  PanelLeftClose,
  PanelLeftOpen,
  Plus,
  Search,
  Trash2,
  TriangleAlert,
  Upload,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Group,
  Panel,
  Separator,
  type PanelImperativeHandle,
} from "react-resizable-panels";
import {
  api,
  ApiError,
  isTextMedia,
  type Entry,
  type SearchHit,
  type SessionInfo,
} from "../api/client";
import EditorPane, { type EditorState } from "../components/EditorPane";
import InfoPanel from "../components/InfoPanel";
import { ThemeToggle } from "../components/ThemeToggle";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogTitle,
  alertActionClass,
  alertCancelClass,
} from "../components/ui/alert-dialog";
import { Button } from "../components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "../components/ui/dialog";
import { Input } from "../components/ui/input";
import { Tooltip } from "../components/ui/tooltip";
import { cycleLanguage } from "../i18n";
import { cn } from "../lib/utils";

/**
 * Main workspace (design §13.1) in the workbench shell shape: one header row,
 * then a resizable three-column group — file tree, editor, info panel. Tight
 * 6px gutters; each column is its own card on a quiet canvas.
 */

type FileNode =
  | { type: "folder"; name: string; path: string; children: FileNode[] }
  | { type: "file"; entry: Entry };

type FlatRow =
  | { kind: "folder"; node: Extract<FileNode, { type: "folder" }>; depth: number }
  | { kind: "file"; entry: Entry; depth: number };

function buildTree(files: Entry[]): FileNode[] {
  const root: Extract<FileNode, { type: "folder" }> = { type: "folder", name: "", path: "", children: [] };
  const folders = new Map<string, Extract<FileNode, { type: "folder" }>>([["", root]]);
  const folderOf = (dir: string) => {
    const found = folders.get(dir);
    if (found) return found;
    const segments = dir.split("/");
    const name = segments[segments.length - 1];
    const parentDir = segments.slice(0, -1).join("/");
    const parent = folderOf(parentDir);
    const node: Extract<FileNode, { type: "folder" }> = { type: "folder", name, path: dir, children: [] };
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

function flattenTree(nodes: FileNode[], collapsed: Set<string>, depth = 0, out: FlatRow[] = []): FlatRow[] {
  for (const node of nodes) {
    if (node.type === "folder") {
      out.push({ kind: "folder", node, depth });
      // Folders start expanded; only explicitly collapsed ones hide children.
      if (!collapsed.has(node.path)) flattenTree(node.children, collapsed, depth + 1, out);
    } else {
      out.push({ kind: "file", entry: node.entry, depth });
    }
  }
  return out;
}

export default function Workspace({
  session,
  onLogout,
}: {
  session: SessionInfo;
  onLogout: () => void;
}) {
  const { t } = useTranslation("workspace");
  const { t: tCommon } = useTranslation("common");
  const [files, setFiles] = useState<Entry[]>([]);
  const [filesTruncated, setFilesTruncated] = useState(false);
  const [showTrash, setShowTrash] = useState(false);
  const [editor, setEditor] = useState<EditorState>({ kind: "empty" });
  const [searchQuery, setSearchQuery] = useState("");
  const [searchMode, setSearchMode] = useState<"literal" | "fts">("literal");
  const [searchResults, setSearchResults] = useState<SearchHit[] | null>(null);
  const [searchIncomplete, setSearchIncomplete] = useState(false);
  const [searchBuilding, setSearchBuilding] = useState(false);
  const [searching, setSearching] = useState(false);
  const [banner, setBanner] = useState<string | null>(null);
  const [showNewFile, setShowNewFile] = useState(false);
  const [newFilePath, setNewFilePath] = useState("");
  const [showTrashConfirm, setShowTrashConfirm] = useState(false);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const sidebarPanel = useRef<PanelImperativeHandle | null>(null);
  // Batch selection: entering selection mode shows checkboxes; row clicks
  // toggle instead of opening. Batch actions run sequentially per file.
  const [selecting, setSelecting] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [batchRunning, setBatchRunning] = useState(false);
  // Folders are expanded unless explicitly collapsed (new folders appear open).
  const [collapsedFolders, setCollapsedFolders] = useState<Set<string>>(new Set());
  // Opening another file with unsaved/conflicting text in the editor requires
  // an explicit discard — the confirm dialog reuses the trash pattern.
  const [pendingOpen, setPendingOpen] = useState<string | null>(null);
  const editorRef = useRef(editor);
  editorRef.current = editor;

  const treeRows = useMemo(
    () => flattenTree(buildTree(files), collapsedFolders),
    [files, collapsedFolders],
  );

  const toggleSidebar = () => {
    const panel = sidebarPanel.current;
    if (!panel) return;
    if (panel.isCollapsed()) {
      panel.expand();
      setSidebarCollapsed(false);
    } else {
      panel.collapse();
      setSidebarCollapsed(true);
    }
  };

  const toggleFolder = (path: string) => {
    setCollapsedFolders((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };

  const fail = useCallback(
    (error: unknown) => {
      // An expired session has no in-page recovery: back to login.
      if (error instanceof ApiError && error.status === 401) {
        setBanner(t("bannerAuthLost"));
        onLogout();
        return;
      }
      setBanner(error instanceof Error ? error.message : String(error));
    },
    [onLogout, t],
  );

  const refreshFiles = useCallback(async () => {
    // Walk pages up to a sane bound; anything left is surfaced as truncated
    // instead of silently hiding files (design §11.1: no fake completion).
    let items: Entry[] = [];
    let cursor: string | null = null;
    let guard = 0;
    do {
      const page = await api.listFiles({
        source: "store",
        include_deleted: showTrash,
        limit: 100,
        cursor,
      });
      items = items.concat(page.items);
      cursor = page.next_cursor;
      guard += 1;
    } while (cursor && guard < 20);
    setFiles(items);
    setFilesTruncated(Boolean(cursor));
  }, [showTrash]);

  useEffect(() => {
    void refreshFiles();
  }, [refreshFiles]);

  // Leaving trash view (or the selection mode) resets stale selections.
  useEffect(() => {
    setSelected(new Set());
  }, [showTrash]);

  const requestOpen = (path: string) => {
    if (selecting) {
      setSelected((prev) => {
        const next = new Set(prev);
        if (next.has(path)) next.delete(path);
        else next.add(path);
        return next;
      });
      return;
    }
    const current = editorRef.current;
    // `saving` counts too: keystrokes typed during the flight have not been
    // sent yet and would be dropped without a word.
    if (
      current.kind === "dirty" ||
      current.kind === "conflict" ||
      current.kind === "error" ||
      current.kind === "saving"
    ) {
      setPendingOpen(path);
      return;
    }
    void openFile(path);
  };

  const openFile = async (path: string) => {
    setEditor({ kind: "loading", path });
    try {
      // Text-only editor: refuse binary media instead of decoding bytes into
      // mojibake that a later save would corrupt (design §13.3).
      const meta = await api.readMeta(path);
      if (!isTextMedia(meta.media_type)) {
        setBanner(t("bannerBinary", { path, mediaType: meta.media_type }));
        setEditor({ kind: "empty" });
        return;
      }
      const { text, etag } = await api.readText(path);
      // The two calls are separate: if the file moved between them (e.g. a
      // rival replaced it with binary bytes), reopen once against the truth.
      const latest = await api.readMeta(path);
      if (latest.revision !== etag) {
        if (!isTextMedia(latest.media_type)) {
          setBanner(t("bannerReplacedBinary", { path }));
          setEditor({ kind: "empty" });
          return;
        }
        const fresh = await api.readText(path);
        setEditor({ kind: "clean", path, text: fresh.text, baseText: fresh.text, baseRevision: fresh.etag });
        return;
      }
      setEditor({ kind: "clean", path, text, baseText: text, baseRevision: etag });
    } catch (error) {
      if (error instanceof ApiError && error.status === 410) {
        setBanner(t("bannerInTrash", { path }));
        setEditor({ kind: "empty" });
        return;
      }
      fail(error);
      setEditor({ kind: "empty" });
    }
  };

  const newFile = async () => {
    const path = newFilePath.trim();
    if (!path) return;
    const current = editorRef.current;
    if (current.kind === "dirty" || current.kind === "conflict" || current.kind === "error") {
      setBanner(t("blockedNewFile"));
      return;
    }
    try {
      const result = await api.saveText(path, "", { mode: "create" });
      setNewFilePath("");
      setShowNewFile(false);
      await refreshFiles();
      setEditor({ kind: "clean", path: result.path, text: "", baseText: "", baseRevision: result.revision });
    } catch (error) {
      fail(error);
    }
  };

  const trashCurrent = async () => {
    const current = editorRef.current;
    if (current.kind !== "clean" && current.kind !== "dirty") return;
    setShowTrashConfirm(false);
    try {
      await api.trash(current.path, current.baseRevision);
      setEditor({ kind: "empty" });
      await refreshFiles();
    } catch (error) {
      fail(error);
    }
  };

  const uploadAttachment = async (file: File) => {
    const path = `attachments/${file.name}`;
    try {
      const buffer = await file.arrayBuffer();
      await api.saveBinary(
        path,
        buffer,
        { mode: "create" },
        file.type || "application/octet-stream",
      );
      setBanner(t("bannerUploaded", { path }));
      await refreshFiles();
    } catch (error) {
      fail(error);
    }
  };

  const runSearch = async () => {
    if (!searchQuery.trim()) return;
    setSearching(true);
    try {
      const page = await api.search({ q: searchQuery, mode: searchMode });
      let items = page.items;
      let cursor = page.next_cursor;
      // The UI walks literal windows for the user up to a sane bound; any
      // remaining cursor is surfaced as "未翻完" instead of faking completion.
      let guard = 0;
      while (cursor && guard < 20) {
        const next = await api.search({ q: searchQuery, mode: searchMode, cursor });
        items = items.concat(next.items);
        cursor = next.next_cursor;
        guard += 1;
      }
      setSearchResults(items);
      setSearchIncomplete(Boolean(cursor) || !page.complete);
      setSearchBuilding(page.index_state === "building");
    } catch (error) {
      fail(error);
    } finally {
      setSearching(false);
    }
  };

  const restoreOne = async (path: string): Promise<boolean> => {
    // The tombstone itself has no content: restore the newest contentful
    // revision reachable from it (design §8.1: restore takes a live source).
    const history = await api.history(path);
    const anchor = history.anchor_revision;
    let source: string | null = null;
    let cursor: string | null = null;
    let pages = 0;
    do {
      const page = await api.history(path, cursor ?? undefined);
      const newestContentful = page.items.find((rev) => rev.action !== "trash");
      if (newestContentful) {
        source = newestContentful.revision;
        break;
      }
      cursor = page.next_cursor;
      pages += 1;
    } while (cursor && pages < 10);
    if (!source) return false;
    await api.restore(path, source, anchor);
    return true;
  };

  const restoreFromTrash = async (path: string) => {
    try {
      const ok = await restoreOne(path);
      if (!ok) {
        setBanner(t("bannerNoHistory", { path }));
        return;
      }
      setBanner(t("bannerRestored", { path }));
      await refreshFiles();
    } catch (error) {
      fail(error);
    }
  };

  const exitSelecting = () => {
    setSelecting(false);
    setSelected(new Set());
  };

  const runBatch = async (kind: "trash" | "restore") => {
    if (selected.size === 0 || batchRunning) return;
    setBatchRunning(true);
    let done = 0;
    const failed: string[] = [];
    for (const path of selected) {
      try {
        if (kind === "trash") {
          const meta = await api.readMeta(path);
          await api.trash(path, meta.revision);
        } else {
          const ok = await restoreOne(path);
          if (!ok) throw new Error(t("bannerNoHistory", { path }));
        }
        done += 1;
      } catch {
        failed.push(path);
      }
    }
    setBatchRunning(false);
    setSelected(new Set());
    setSelecting(false);
    await refreshFiles();
    if (failed.length > 0) {
      setBanner(
        t("batchPartial", {
          done,
          failed: failed.length,
          detail: failed.slice(0, 3).join("、"),
        }),
      );
    } else {
      setBanner(
        kind === "trash"
          ? t("batchTrashDone", { count: done })
          : t("batchRestoreDone", { count: done }),
      );
    }
  };

  const allSelected = selected.size > 0 && selected.size === files.length;

  const canTrash = editor.kind === "clean" || editor.kind === "dirty";

  return (
    <div className="flex h-full min-h-0 flex-col gap-1.5 bg-background p-1.5">
      <header className="flex h-11 shrink-0 items-center gap-1.5 rounded-lg border bg-card px-2 shadow-card">
        <span className="flex items-center gap-1.5 pl-1 pr-0.5 text-sm font-semibold">
          <Brain className="size-4 text-primary" />
          Brains
        </span>
        <Tooltip content={sidebarCollapsed ? t("expandSidebar") : t("collapseSidebar")}>
          <Button
            variant="ghost"
            size="icon"
            aria-pressed={sidebarCollapsed}
            onClick={toggleSidebar}
          >
            {sidebarCollapsed ? <PanelLeftOpen /> : <PanelLeftClose />}
          </Button>
        </Tooltip>

        <div className="relative ml-0.5 w-full max-w-md">
          <Search className="pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            data-testid="search-box"
            placeholder={t("searchPlaceholder")}
            className="h-8 pr-16 pl-8"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void runSearch();
            }}
          />
          <div className="absolute right-1 top-1/2 flex -translate-y-1/2" role="group">
            <ModeButton
              active={searchMode === "literal"}
              testid="mode-literal"
              onClick={() => setSearchMode("literal")}
            >
              {t("modeLiteral")}
            </ModeButton>
            <ModeButton
              active={searchMode === "fts"}
              testid="mode-fts"
              onClick={() => setSearchMode("fts")}
            >
              {t("modeFts")}
            </ModeButton>
          </div>
        </div>

        <div className="ml-auto flex items-center gap-1">
          <Button data-testid="new-file" onClick={() => setShowNewFile(true)}>
            <Plus />
            {tCommon("newFile")}
          </Button>
          <label>
            <Button variant="outline" className="cursor-pointer">
              <Upload />
              {tCommon("upload")}
              <input
                type="file"
                data-testid="upload-input"
                hidden
                onChange={(e) => {
                  const file = e.target.files?.[0];
                  if (file) void uploadAttachment(file);
                  e.target.value = "";
                }}
              />
            </Button>
          </label>
          <Button
            variant={showTrash ? "secondary" : "outline"}
            aria-pressed={showTrash}
            data-testid="toggle-trash"
            onClick={() => setShowTrash((value) => !value)}
          >
            {showTrash ? <ArchiveRestore /> : <Trash2 />}
            {showTrash ? tCommon("exitTrash") : tCommon("trash")}
          </Button>
          <ThemeToggle />
          <Tooltip content={tCommon("languageLabel")}>
            <Button
              variant="ghost"
              size="icon"
              aria-label="language"
              onClick={() => cycleLanguage()}
            >
              <Languages />
            </Button>
          </Tooltip>
          <span className="ml-0.5 rounded-full bg-muted px-2 py-1 text-[10.5px] text-muted-foreground">
            {session.actor}
          </span>
          <Button variant="ghost" data-testid="logout" onClick={onLogout}>
            <LogOut />
            {tCommon("logout")}
          </Button>
        </div>
      </header>

      {banner && (
        <div className="pointer-events-none fixed inset-x-0 top-14 z-40 flex justify-center px-4">
          <div
            className="pointer-events-auto flex max-w-xl items-start gap-2 rounded-lg border bg-card px-3 py-2 text-xs shadow-lg"
            data-testid="banner"
            role="status"
          >
            <TriangleAlert className="mt-0.5 size-3.5 shrink-0 text-warning" />
            <span className="min-w-0 flex-1 leading-relaxed">{banner}</span>
            <button
              onClick={() => setBanner(null)}
              aria-label={tCommon("close")}
              className="rounded p-0.5 text-muted-foreground hover:bg-muted hover:text-foreground"
            >
              <X className="size-3.5" />
            </button>
          </div>
        </div>
      )}

      <Group orientation="horizontal" className="min-h-0 flex-1 gap-1.5">
        <Panel
          panelRef={sidebarPanel}
          collapsible
          collapsedSize="0"
          defaultSize="24"
          minSize="16"
          maxSize="45"
        >
          <div className="flex h-full min-h-0 flex-col gap-1.5">
            {searchResults && (
              <div
                className="flex min-h-0 flex-col overflow-hidden rounded-lg border bg-card shadow-card"
                data-testid="search-results"
              >
                <div className="flex items-center gap-2 border-b px-2.5 py-1.5">
                  <Search className="size-3.5 shrink-0 text-muted-foreground" />
                  <h3 className="min-w-0 flex-1 truncate text-xs font-semibold">
                    {t("searchResultsTitle", {
                      query: searchQuery,
                      count: searchResults.length,
                    })}
                    {searchIncomplete && (
                      <span className="font-normal text-destructive">
                        {t("searchIncomplete")}
                      </span>
                    )}
                    {searchBuilding && (
                      <span className="font-normal text-muted-foreground">
                        {t("searchBuilding")}
                      </span>
                    )}
                  </h3>
                  {searching && (
                    <LoaderCircle className="size-3.5 shrink-0 animate-spin text-muted-foreground" />
                  )}
                  <button
                    aria-label={t("closeSearch")}
                    className="rounded p-0.5 text-muted-foreground hover:bg-muted hover:text-foreground"
                    onClick={() => setSearchResults(null)}
                  >
                    <X className="size-3.5" />
                  </button>
                </div>
                <ul className="min-h-0 flex-1 overflow-auto p-1">
                  {searchResults.map((hit) => (
                    <li key={hit.path + hit.indexed_revision} className="rounded-md p-1 hover:bg-muted/70">
                      <button
                        className="w-full truncate text-left font-mono text-xs hover:text-primary"
                        onClick={() => requestOpen(hit.path)}
                        title={hit.path}
                      >
                        {hit.path}
                      </button>
                      <p className="mt-0.5 line-clamp-2 text-[11px] text-muted-foreground">
                        {hit.snippet.slice(0, 160)}
                      </p>
                    </li>
                  ))}
                </ul>
              </div>
            )}

            <div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-lg border bg-card shadow-card">
              <div className="flex items-center gap-1.5 border-b px-2.5 py-1.5">
                <h2 className="flex-1 text-xs font-semibold">
                  {showTrash ? t("panelTrash") : t("panelFiles")}
                  <span className="ml-1.5 font-normal text-muted-foreground">
                    {files.length}
                  </span>
                </h2>
                {filesTruncated && (
                  <span
                    className="text-[10.5px] text-muted-foreground"
                    data-testid="files-truncated"
                  >
                    {t("truncated")}
                  </span>
                )}
                {files.length > 0 && !batchRunning && (
                  <Tooltip content={selecting ? t("exitSelect") : t("select")}>
                    <Button
                      variant={selecting ? "secondary" : "ghost"}
                      size="iconSm"
                      aria-pressed={selecting}
                      onClick={() => (selecting ? exitSelecting() : setSelecting(true))}
                    >
                      {selecting ? <X /> : <ListChecks />}
                    </Button>
                  </Tooltip>
                )}
              </div>

              {selecting && (
                <div className="flex items-center gap-2 border-b bg-muted/30 px-2.5 py-1.5">
                  <label className="flex cursor-pointer items-center gap-1.5 text-[11px] text-muted-foreground">
                    <input
                      type="checkbox"
                      className="size-3.5 accent-[var(--primary)]"
                      checked={allSelected}
                      onChange={() => {
                        setSelected(
                          allSelected ? new Set() : new Set(files.map((f) => f.path)),
                        );
                      }}
                    />
                    {t("selectAll")}
                  </label>
                  <span className="text-[11px] text-muted-foreground">
                    {t("selectedCount", { count: selected.size })}
                  </span>
                  <div className="ml-auto flex items-center gap-1">
                    {batchRunning && (
                      <LoaderCircle className="size-3.5 animate-spin text-muted-foreground" />
                    )}
                    <Button
                      size="sm"
                      variant={showTrash ? "default" : "destructive"}
                      disabled={selected.size === 0 || batchRunning}
                      onClick={() => void runBatch(showTrash ? "restore" : "trash")}
                    >
                      {showTrash ? <ArchiveRestore /> : <Trash2 />}
                      {showTrash ? t("batchRestore") : t("batchTrash")}
                    </Button>
                  </div>
                </div>
              )}

              <div className="min-h-0 flex-1 overflow-auto p-1" data-testid="file-list">
                {files.length === 0 && (
                  <div className="flex flex-col items-center gap-2 px-3 py-8 text-muted-foreground">
                    <FolderOpen className="size-5 opacity-60" />
                    <p className="text-xs">{t("noFiles")}</p>
                  </div>
                )}
                <ul className="space-y-px">
                  {treeRows.map((row) =>
                    row.kind === "folder" ? (
                      <li key={`dir:${row.node.path}`}>
                        <button
                          className="flex w-full items-center gap-1.5 rounded-md px-1.5 py-1 text-left transition-colors hover:bg-muted/70"
                          style={{ paddingLeft: `${row.depth * 12 + 6}px` }}
                          onClick={() => toggleFolder(row.node.path)}
                          aria-expanded={!collapsedFolders.has(row.node.path)}
                          title={t(
                            collapsedFolders.has(row.node.path)
                              ? "folderExpand"
                              : "folderCollapse",
                            { name: row.node.name },
                          )}
                        >
                          <ChevronRight
                            className={cn(
                              "size-3 shrink-0 text-muted-foreground transition-transform",
                              !collapsedFolders.has(row.node.path) && "rotate-90",
                            )}
                          />
                          {!collapsedFolders.has(row.node.path) ? (
                            <FolderOpen className="size-3.5 shrink-0 text-muted-foreground" />
                          ) : (
                            <Folder className="size-3.5 shrink-0 text-muted-foreground" />
                          )}
                          <span className="min-w-0 flex-1 truncate text-xs font-medium">
                            {row.node.name}
                          </span>
                        </button>
                      </li>
                    ) : (
                      <li key={row.entry.path} className="group relative">
                        <button
                          className={cn(
                            "flex w-full items-center gap-1.5 rounded-md px-1.5 py-1 text-left transition-colors",
                            selected.has(row.entry.path)
                              ? "bg-primary/13"
                              : "hover:bg-primary/9",
                            "focus-visible:outline-2 focus-visible:outline-ring",
                          )}
                          style={{ paddingLeft: `${row.depth * 12 + 6}px` }}
                          onClick={() => requestOpen(row.entry.path)}
                          title={row.entry.path}
                        >
                          {selecting && (
                            <input
                              type="checkbox"
                              className="size-3.5 shrink-0 accent-[var(--primary)]"
                              checked={selected.has(row.entry.path)}
                              onClick={(e) => e.stopPropagation()}
                              onChange={() => requestOpen(row.entry.path)}
                            />
                          )}
                          <FileText className="size-3.5 shrink-0 text-muted-foreground" />
                          <span className="file-path min-w-0 flex-1 truncate font-mono text-xs">
                            {selecting
                              ? row.entry.path.split("/").pop()
                              : row.entry.path}
                          </span>
                          {row.entry.state === "trashed" && (
                            <span className="shrink-0 rounded-full bg-warning/12 px-1.5 py-0.5 text-[9.5px] text-warning">
                              {t("trashedBadge")}
                            </span>
                          )}
                        </button>
                        {row.entry.state === "trashed" && !selecting && (
                          <button
                            className="restore-button absolute bottom-1/2 right-1.5 translate-y-1/2 rounded-md bg-card/95 p-1 text-muted-foreground opacity-0 shadow-card transition-opacity hover:text-primary focus-visible:opacity-100 focus-visible:outline-2 focus-visible:outline-ring group-hover:opacity-100"
                            aria-label={t("restoreAria", { path: row.entry.path })}
                            title={t("restoreTooltip")}
                            onClick={(e) => {
                              e.stopPropagation();
                              void restoreFromTrash(row.entry.path);
                            }}
                          >
                            <ArchiveRestore className="size-3.5" />
                          </button>
                        )}
                      </li>
                    ),
                  )}
                </ul>
              </div>
            </div>
          </div>
        </Panel>

        <ResizeSeparator />

        <Panel minSize="35">
          <EditorPane
            state={editor}
            onChange={setEditor}
            onStateChange={setEditor}
            onAuthLost={() => {
              setBanner(t("bannerAuthLost"));
              onLogout();
            }}
            canTrash={canTrash}
            onTrashCurrent={() => setShowTrashConfirm(true)}
          />
        </Panel>

        <ResizeSeparator />

        <Panel defaultSize="22" minSize="15" maxSize="40">
          <InfoPanel
            path={infoPath(editor)}
            revision={infoRevision(editor)}
            onRestore={(revision) => {
              // Restoring replaces the buffer: only a clean editor may lose it
              // silently; dirty/conflict editors must resolve first.
              if (editor.kind !== "clean") {
                setBanner(t("blockedRestore"));
                return;
              }
              void (async () => {
                try {
                  const result = await api.restore(editor.path, revision, editor.baseRevision);
                  const { text, etag } = await api.readText(editor.path);
                  void result;
                  setEditor({
                    kind: "clean",
                    path: editor.path,
                    text,
                    baseText: text,
                    baseRevision: etag,
                  });
                  setBanner(t("bannerRestoredShort"));
                  await refreshFiles();
                } catch (error) {
                  fail(error);
                }
              })();
            }}
          />
        </Panel>
      </Group>

      <Dialog open={showNewFile} onOpenChange={setShowNewFile}>
        <DialogContent data-testid="new-file-dialog">
          <DialogHeader>
            <DialogTitle>{t("newFileDialogTitle")}</DialogTitle>
            <DialogDescription>{t("newFileDialogHint")}</DialogDescription>
          </DialogHeader>
          <div className="px-5 pt-4">
            <Input
              data-testid="new-file-path"
              placeholder={t("newFilePlaceholder")}
              value={newFilePath}
              onChange={(e) => setNewFilePath(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void newFile();
              }}
              autoFocus
            />
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setShowNewFile(false)}>
              {tCommon("cancel")}
            </Button>
            <Button data-testid="new-file-confirm" onClick={() => void newFile()}>
              {tCommon("create")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <AlertDialog open={showTrashConfirm} onOpenChange={setShowTrashConfirm}>
        <AlertDialogContent data-testid="trash-confirm">
          <AlertDialogTitle>{t("trashConfirmTitle")}</AlertDialogTitle>
          <AlertDialogDescription>{t("trashConfirmBody")}</AlertDialogDescription>
          <AlertDialogFooter>
            <AlertDialogCancel className={alertCancelClass()}>
              {tCommon("cancel")}
            </AlertDialogCancel>
            <AlertDialogAction
              data-testid="trash-confirm-yes"
              className={alertActionClass("bg-destructive text-destructive-foreground hover:bg-destructive/90")}
              onClick={() => void trashCurrent()}
            >
              {t("trashConfirmAction")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog open={pendingOpen !== null} onOpenChange={(open) => !open && setPendingOpen(null)}>
        <AlertDialogContent data-testid="open-confirm">
          <AlertDialogTitle>{t("openConfirmTitle")}</AlertDialogTitle>
          <AlertDialogDescription>
            {t("openConfirmBody", { path: pendingOpen ?? "" })}
          </AlertDialogDescription>
          <AlertDialogFooter>
            <AlertDialogCancel className={alertCancelClass()}>
              {tCommon("cancel")}
            </AlertDialogCancel>
            <AlertDialogAction
              data-testid="open-confirm-yes"
              className={alertActionClass("bg-destructive text-destructive-foreground hover:bg-destructive/90")}
              onClick={() => {
                const path = pendingOpen;
                setPendingOpen(null);
                if (path) void openFile(path);
              }}
            >
              {t("openConfirmAction")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

function ModeButton({
  active,
  testid,
  onClick,
  children,
}: {
  active: boolean;
  testid: string;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      data-testid={testid}
      aria-pressed={active}
      onClick={onClick}
      className={cn(
        "rounded-md px-2 py-1 text-[10.5px] font-medium transition-colors",
        active
          ? "bg-primary/12 text-primary ring-1 ring-primary/30"
          : "text-muted-foreground hover:bg-muted hover:text-foreground",
      )}
    >
      {children}
    </button>
  );
}

function ResizeSeparator() {
  return (
    <Separator className="w-0.5 rounded-full bg-transparent transition-colors hover:bg-primary/40 data-[state=dragging]:bg-primary/60" />
  );
}

function infoPath(state: EditorState): string | null {
  return state.kind === "empty" || state.kind === "loading" ? null : state.path;
}

function infoRevision(state: EditorState): string | null {
  switch (state.kind) {
    case "clean":
    case "dirty":
    case "error":
    case "saving":
      return state.baseRevision;
    default:
      return null;
  }
}
