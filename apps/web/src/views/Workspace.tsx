import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Group, Panel, Separator, type PanelImperativeHandle } from "react-resizable-panels";
import {
  api,
  ApiError,
  IS_DEMO,
  isTextMedia,
  mediaTypeForPath,
  type Entry,
  type SessionInfo,
} from "../api/client";
import { TopBar, type UploadIntent } from "../components/shell/top-bar";
import { SettingsDialog } from "../components/shell/settings-dialog";
import { Sidebar, type WorkspaceMode } from "../components/shell/sidebar";
import { FileTree, collectFolders, type TreeActions } from "../components/tree/file-tree";
import { expandEntry } from "../components/tree/selection";
import { CommandPalette } from "../components/search/command-palette";
import EditorPane from "../components/EditorPane";
import { type EditorState } from "../components/editor/editor-core";
import { clearDraft, readDraft, writeDraft } from "../components/editor/editor-settings";
import { PropertiesPanel } from "../components/properties/properties-panel";
import { TrashList } from "../components/trash/trash-list";
import { TrashViewer } from "../components/trash/trash-viewer";
import {
  UploadConflictDialog,
} from "../components/upload/upload-conflict-dialog";
import { UploadProgress } from "../components/upload/upload-progress";
import { useUpload } from "../components/upload/use-upload";
import { UploadInputs } from "../components/upload/upload-inputs";
import { Banner } from "../components/shell/banner";
import { SidebarListSkeleton } from "../components/ui/skeleton";
import { useTrashFlow } from "./use-trash-flow";
import { useClipboardImport } from "./use-clipboard-import";
import { useExternalDrag } from "./use-external-drag";
import { useMoveBatch } from "./use-move-batch";
import { WorkspaceDialogs, type DialogState } from "./workspace-dialogs";
import { MobileLayout } from "./mobile-layout";
import { useFileNavigation } from "./use-file-navigation";
import { useMobileTabSync } from "./use-mobile-tab-sync";

/*
 * Workspace orchestrator (DESIGN.md §61-62): explicit state unions instead of
 * boolean soup — mode (workspace/trash), dialog (discriminated), selection
 * (Set), the on-demand properties panel and the ⌘K palette. All business
 * mutations flow through the conditional-write API client. Trash mutations
 * live in use-trash-flow, clipboard intake in use-clipboard-import and the
 * confirm dialogs in workspace-dialogs.
 */

type SidePanel =
  | { type: "none" }
  // follow: the panel tracks the context selection (editor file in the
  // workspace, trash selection in the trash) instead of a pinned path.
  | { type: "properties"; path: string | null; follow: boolean };

export default function Workspace({
  session,
  onLogout,
}: {
  session: SessionInfo;
  onLogout: (reason?: string) => void;
}) {
  const { t } = useTranslation("workspace");
  const [mode, setMode] = useState<WorkspaceMode>("workspace");
  const [liveFiles, setLiveFiles] = useState<Entry[]>([]);
  const [trashedFiles, setTrashedFiles] = useState<Entry[]>([]);
  const [filesTruncated, setFilesTruncated] = useState(false);
  const [editor, setEditor] = useState<EditorState>({ kind: "empty" });
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [renaming, setRenaming] = useState<string | null>(null);
  const [panel, setPanel] = useState<SidePanel>({ type: "none" });
  const propertiesOpen = panel.type === "properties";
  const openProperties = (path: string | null, follow = true) =>
    setPanel({ type: "properties", path, follow });
  const closeProperties = useCallback(() => {
    // Hand-rolled ✕/Esc have no Base-UI focus restore: return focus to
    // whoever opened the panel (the editor/toolbar trigger) when it survives.
    const opener = document.activeElement as HTMLElement | null;
    setPanel({ type: "none" });
    requestAnimationFrame(() => {
      if (opener && document.contains(opener)) {
        opener.focus();
        return;
      }
      // ✕ was focused (it unmounts with the panel): back to the document.
      (document.querySelector(".cm-content") as HTMLElement | null)?.focus();
    });
  }, []);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [paletteTags, setPaletteTags] = useState<string[]>([]);
  const openTag = useCallback((tag: string) => {
    setPaletteTags([tag]);
    setPaletteOpen(true);
  }, []);
  const [searchMode, setSearchMode] = useState<"literal" | "fts" | "hybrid">("literal");
  const [dialog, setDialog] = useState<DialogState>(null);
  const [selecting, setSelecting] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [batchRunning, setBatchRunning] = useState(false);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  // First vault walk: show sidebar skeletons instead of an empty list.
  const [listLoading, setListLoading] = useState(true);
  const sidebarPanel = useRef<PanelImperativeHandle | null>(null);
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const folderInputRef = useRef<HTMLInputElement | null>(null);
  const editorRef = useRef(editor);
  editorRef.current = editor;
  const changeEditor = useCallback((next: EditorState) => {
    const previous = editorRef.current;
    if (next.kind !== "empty" && (previous.kind === "empty" || next.path !== previous.path)) {
      setSelected(new Set([next.path]));
    }
    editorRef.current = next;
    setEditor(next);
  }, []);
  const [banner, setBanner] = useState<string | null>(null);

  const folders = useMemo(() => collectFolders(liveFiles), [liveFiles]);
  const folderSet = useMemo(() => new Set(folders), [folders]);
  // Reference completions for the editor ([[wiki]], [](/![]() links).
  const completionFiles = useMemo(
    () =>
      liveFiles.map((entry) => ({
        path: entry.path,
        binary: !isTextMedia(entry.media_type ?? mediaTypeForPath(entry.path)),
      })),
    [liveFiles],
  );
  const shownFiles = mode === "workspace" ? liveFiles : trashedFiles;

  // Stable identity matters: EditorPane reconfigures completions when these change.
  const fail = useCallback(
    (error: unknown) => {
      // An expired session has no in-page recovery: back to login.
      if (error instanceof ApiError && error.status === 401) {
        onLogout(t("bannerAuthLost"));
        return;
      }
      setBanner(error instanceof Error ? error.message : String(error));
    },
    [t, onLogout],
  );

  const refreshFiles = async () => {
    // One walk with tombstones included: the tree shows live entries, the
    // trash view shows tombstones. Anything past the sane page bound is
    // surfaced as truncated, never silently hidden (design §11.1).
    let items: Entry[] = [];
    let cursor: string | null = null;
    let guard = 0;
    do {
      const page = await api.listFiles({
        source: "store",
        include_deleted: true,
        limit: 100,
        cursor,
      });
      items = items.concat(page.items);
      cursor = page.next_cursor;
      guard += 1;
    } while (cursor && guard < 20);
    setLiveFiles(items.filter((item) => item.state === "live"));
    setTrashedFiles(items.filter((item) => item.state === "trashed"));
    setFilesTruncated(Boolean(cursor));
    setListLoading(false);
  };

  useEffect(() => {
    void refreshFiles().catch(fail);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const upload = useUpload({
    files: useMemo(
      () =>
        liveFiles.map((entry) => ({
          path: entry.path,
          revision: entry.revision,
          sha256: entry.sha256,
        })),
      [liveFiles],
    ),
    refresh: refreshFiles,
    onFail: fail,
  });
  const { runUpload, importFromClipboard } = useClipboardImport({ upload, mode, t });
  const { dragging, overlayHandlers } = useExternalDrag({ mode, upload, runUpload });

  const openUploadIntent = (intent: UploadIntent) => {
    if (intent === "files") fileInputRef.current?.click();
    else if (intent === "folder") folderInputRef.current?.click();
    else void importFromClipboard();
  };

  // ---- Open / guard ------------------------------------------------------

  const openRunRef = useRef(0);
  const openFile = useCallback(async (path: string) => {
    const runId = openRunRef.current + 1;
    openRunRef.current = runId;
    const stale = () => openRunRef.current !== runId;
    setSelected(new Set([path]));
    setEditor({ kind: "loading", path });
    try {
      // Text-only editor: refuse binary media instead of decoding bytes into
      // mojibake that a later save would corrupt (design §13.3).
      const meta = await api.readMeta(path);
      if (stale()) return;
      if (!isTextMedia(meta.media_type)) {
        setBanner(t("bannerBinary", { path, mediaType: meta.media_type }));
        setEditor({ kind: "empty" });
        return;
      }
      const { text, etag } = await api.readText(path);
      if (stale()) return;
      // The two calls are separate: if the file moved between them, reopen
      // once against the truth.
      const latest = await api.readMeta(path);
      if (stale()) return;
      if (latest.revision !== etag) {
        if (!isTextMedia(latest.media_type)) {
          setBanner(t("bannerReplacedBinary", { path }));
          setEditor({ kind: "empty" });
          return;
        }
        const fresh = await api.readText(path);
        if (stale()) return;
        setEditor({ kind: "clean", path, text: fresh.text, baseText: fresh.text, baseRevision: fresh.etag });
        return;
      }
      setEditor({ kind: "clean", path, text, baseText: text, baseRevision: etag });
    } catch (error) {
      if (stale()) return;
      if (error instanceof ApiError && error.status === 410) {
        setBanner(t("bannerInTrash", { path }));
        setEditor({ kind: "empty" });
        return;
      }
      fail(error);
      setEditor({ kind: "empty" });
    }
  }, [t, fail]);

  const isFileOpen = (path: string) => editorRef.current.kind !== "empty" && editorRef.current.path === path;
  const navigation = useFileNavigation({
    open: (path) => {
      setMode("workspace");
      if (path) {
        if (!isFileOpen(path)) void openFile(path);
      } else {
        openRunRef.current += 1;
        changeEditor({ kind: "empty" });
        setSelected(new Set());
      }
    },
    isOpen: isFileOpen,
    isDirty: () => ["dirty", "conflict", "error", "saving"].includes(editorRef.current.kind),
    onBlocked: (path) => setDialog({ type: "discard-open", path: path ?? "" }),
  });
  const navigationRef = useRef(navigation);
  navigationRef.current = navigation;
  const requestOpen = useCallback((path: string) => navigationRef.current.navigate(path), []);

  // ---- Tree actions ------------------------------------------------------

  // The sidebar listing is refreshed on mutations made OUTSIDE the editor;
  // an explicit editor save updates only the editor state, so for the open
  // file its baseRevision is the freshest known revision.
  const entryRevision = (path: string) => {
    const current = editorRef.current;
    if (
      current.kind !== "empty" &&
      current.kind !== "loading" &&
      current.kind !== "conflict" &&
      current.path === path
    ) {
      return current.baseRevision;
    }
    return liveFiles.find((entry) => entry.path === path)?.revision;
  };

  const treeActions: TreeActions = {
    onOpen: requestOpen,
    onStartRename: (path) => setRenaming(path || null),
    onCancelRename: () => setRenaming(null),
    onCommitRename: (from, to) => {
      setRenaming(null);
      void (async () => {
        const revision = entryRevision(from);
        if (!revision) {
          toast.error(t("renameFailed", { path: from }));
          return;
        }
        try {
          await api.move(from, to, revision);
          toast.success(t("renameDone", { from, to }));
          await refreshFiles();
          // Drafts belong to the content: follow the rename so the old path
          // can't resurrect it.
          const draft = readDraft(from);
          if (draft) {
            clearDraft(from);
            writeDraft(to, draft.text);
          }
          // Batch selection follows the rename (whole subtree for folders),
          // or a subsequent Delete would target dead paths.
          setSelected((prev) => {
            const next = new Set<string>();
            for (const one of prev) {
              if (one === from) next.add(to);
              else if (one.startsWith(`${from}/`)) next.add(`${to}/${one.slice(from.length + 1)}`);
              else next.add(one);
            }
            return next;
          });
          const current = editorRef.current;
          if (current.kind !== "empty" && current.kind !== "loading" && current.path === from) {
            // Dirty/saving buffers must not be silently reloaded from disk:
            // same discard guard as opening another file.
            requestOpen(to);
          }
        } catch (error) {
          fail(error);
        }
      })();
    },
    onTrash: (paths) => {
      const expanded = [...new Set(paths.flatMap((path) => expandEntry(liveFiles, folderSet, path)))];
      if (expanded.length === 0) return;
      setDialog({ type: "trash", paths: expanded });
    },
    onDuplicate: (path) => {
      void (async () => {
        const revision = entryRevision(path);
        if (!revision) return;
        const slash = path.lastIndexOf("/");
        const dir = slash >= 0 ? path.slice(0, slash + 1) : "";
        const name = slash >= 0 ? path.slice(slash + 1) : path;
        const dot = name.lastIndexOf(".");
        const stem = dot > 0 ? name.slice(0, dot) : name;
        const ext = dot > 0 ? name.slice(dot) : "";
        const taken = new Set([...liveFiles, ...trashedFiles].map((entry) => entry.path));
        let target = `${dir}${stem}-copy${ext}`;
        for (let i = 1; taken.has(target); i += 1) {
          target = `${dir}${stem}-copy-${i}${ext}`;
        }
        try {
          await api.copy(path, target, revision);
          toast.success(t("duplicateDone", { target }));
          await refreshFiles();
        } catch (error) {
          fail(error);
        }
      })();
    },
    onCopyPath: (path) => {
      void navigator.clipboard.writeText(path).then(() => toast.success(t("copyPathDone")));
    },
    onCopyNetPath: (path) => {
      const url = api.fileUrl(path);
      void navigator.clipboard.writeText(url).then(() => toast.success(t(IS_DEMO ? "demoLinkCopied" : "copyNetPathDone")));
    },
    onNewDocumentIn: (folder) => setDialog({ type: "new-document", folder }),
    onCreateFolderIn: (folder) => setDialog({ type: "new-folder", folder }),
    onDropMove: (entries, into) => moveBatch(entries, into),
    onProperties: (path) => openProperties(path),
    onMove: (targets) => setDialog({ type: "move", targets }),
  };

  // ---- Batch move (hook) ---------------------------------------------------

  const moveBatch = useMoveBatch({
    liveFiles,
    entryRevision,
    refreshFiles,
    fail,
    requestOpen,
    setBatchRunning,
    clearSelection: () => setSelected(new Set()),
    isEditorPath: (path) => {
      const current = editorRef.current;
      return current.kind !== "empty" && current.kind !== "loading" && current.path === path;
    },
    t,
  });

  // ---- Trash flow (hook) ---------------------------------------------------

  const trashFlow = useTrashFlow({
    trashedFiles,
    refreshFiles,
    fail,
    setBanner,
    closeDialog: () => setDialog(null),
    setBatchRunning,
    entryRevision,
    onTrashed: (paths) => {
      setSelected(new Set());
      const current = editorRef.current;
      if (current.kind !== "empty" && current.kind !== "loading" && paths.includes(current.path)) {
        setEditor({ kind: "empty" });
      }
    },
    t,
  });

  // ---- New document / move confirm ---------------------------------------

  const newFile = async (path: string) => {
    if (!path) return;
    const current = editorRef.current;
    // `saving` included: keystrokes buffered during the flight would be
    // orphaned once the editor rebinds to the freshly created file.
    if (current.kind === "dirty" || current.kind === "conflict" || current.kind === "error" || current.kind === "saving") {
      setBanner(t("blockedNewFile"));
      return;
    }
    try {
      const result = await api.saveText(path, "", { mode: "create" });
      setDialog(null);
      await refreshFiles();
      requestOpen(result.path);
    } catch (error) {
      fail(error);
    }
  };

  const createFolder = async (rawPath: string) => {
    const raw = rawPath.trim().replace(/\/+/g, "/").replace(/\/+$/, "");
    if (!raw) return;
    try {
      await api.saveText(`${raw}/.keep`, "", { mode: "create" });
      setDialog(null);
      await refreshFiles();
      toast.success(t("folderCreated", { path: raw }));
    } catch (error) {
      fail(error);
    }
  };

  const confirmMove = (destination: string, targets: string[]) => {
    if (!destination || targets.length === 0) return;
    setDialog(null);
    // "folder/" entries move as folders so children keep their structure.
    const entries = targets.map((raw) => ({
      path: raw,
      isFolder: raw.endsWith("/") || folderSet.has(raw.replace(/\/+$/, "")),
    }));
    moveBatch(entries, destination);
  };

  // ---- Properties / restore revision --------------------------------------

  const activeEditorPath =
    editor.kind === "empty" || editor.kind === "loading" ? null : editor.path;
  // The properties panel either follows the context selection (editor file
  // in the workspace, trash selection in the trash) or stays pinned.
  const selectedFiles = [...selected].filter((path) => liveFiles.some((file) => file.path === path));
  const workspacePath = selectedFiles.length === 1 ? selectedFiles[0] : selectedFiles.length > 1 ? null : editor.kind === "empty" ? null : editor.path;
  const trashPath = trashFlow.trashChecked.size === 1 ? [...trashFlow.trashChecked][0] : trashFlow.trashChecked.size > 1 ? null : trashFlow.trashSelected;
  const contextPath = mode === "workspace" ? workspacePath : trashPath;
  const propertiesPath = panel.type === "properties" && !panel.follow ? panel.path : contextPath;

  const [mobileTab, setMobileTab, isMobile] = useMobileTabSync(editor, mode, propertiesOpen);
  const dismissProperties = useCallback(() => {
    closeProperties();
    if (isMobile && mobileTab === "info") {
      setMobileTab(mode === "workspace" && activeEditorPath ? "editor" : "files");
    }
  }, [closeProperties, isMobile, mobileTab, setMobileTab, mode, activeEditorPath]);

  const restoreRevision = (path: string, revision: string, baseRevision: string) => {
    if (editorRef.current.kind !== "clean") {
      setBanner(t("blockedRestore"));
      return;
    }
    void (async () => {
      try {
        await api.restore(path, revision, baseRevision);
        const { text, etag } = await api.readText(path);
        setEditor({ kind: "clean", path, text, baseText: text, baseRevision: etag });
        // Success confirmation rides the toast channel; banner stays for
        // blocking/system-level states only.
        toast.success(t("bannerRestoredShort"));
        await refreshFiles();
      } catch (error) {
        fail(error);
      }
    })();
  };

  // ---- Global shortcuts ----------------------------------------------------

  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      const meta = event.metaKey || event.ctrlKey;
      // A modal dialog owns the keyboard: ⌘K/⌘I/⌘N must not fight it (⌘N
      // over the new-file dialog would wipe the path being typed).
      if (meta && dialog) return;
      if (meta && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setPaletteOpen((open) => !open);
      } else if (meta && event.key.toLowerCase() === "i") {
        event.preventDefault();
        if (isMobile ? mobileTab === "info" : propertiesOpen) dismissProperties();
        else {
          openProperties(contextPath, true);
          if (isMobile) setMobileTab("info");
        }
      } else if (meta && event.key.toLowerCase() === "n") {
        event.preventDefault();
        setDialog({ type: "new-document", folder: "" });
      } else if (
        !dialog &&
        !paletteOpen &&
        (event.key === "Delete" || event.key === "Backspace") &&
        selected.size > 0
      ) {
        // A modal owns the keyboard: Delete here would SWAP the open confirm
        // for the trash dialog and the next Enter would confirm the wrong
        // thing.
        // Batch trash from the keyboard (Backspace = the macOS delete key) —
        // but never while typing in the editor or a form field.
        const target = event.target as HTMLElement | null;
        const typing =
          target?.isContentEditable ||
          target?.tagName === "INPUT" ||
          target?.tagName === "TEXTAREA";
        if (!typing) {
          event.preventDefault();
          treeActions.onTrash([...selected]);
        }
      } else if (event.key === "Escape" && !dialog && !paletteOpen && !event.defaultPrevented) {
        // Esc belongs to the layer that consumed it first (find panel,
        // palette, menus all preventDefault); only a "free" Esc closes the
        // properties panel and clears the batch selection.
        const target = event.target as HTMLElement | null;
        const inChrome =
          target?.isContentEditable ||
          target?.closest("input, textarea, .cm-content, .cm-panel") != null;
        if (!inChrome) {
          dismissProperties();
          setSelected(new Set());
        }
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [propertiesOpen, contextPath, selected, treeActions, dialog, paletteOpen, dismissProperties, openProperties, isMobile, mobileTab, setMobileTab]);

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

  const canTrash = editor.kind === "clean" || editor.kind === "dirty";

  // The three panes are defined once; desktop lays them out side by side,
  // mobile swaps them through the bottom tab bar (editor stays mounted).
  const sidebarPane = (
    <Sidebar
      mode={mode}
      onModeChange={(next) => {
        setMode(next);
        setSelected(new Set());
        setRenaming(null);
        // The properties panel follows the OTHER mode's context — carrying it
        // over would only show a stale "no selection" column.
        setPanel({ type: "none" });
        trashFlow.setTrashSelected(null);
        trashFlow.setTrashChecked(new Set());
      }}
      fileCount={shownFiles.length}
      truncated={filesTruncated}
      >
        {listLoading ? (
          <SidebarListSkeleton />
        ) : mode === "workspace" ? (
        <FileTree
          files={liveFiles}
          selected={selected}
          activePath={activeEditorPath}
          renamingPath={renaming}
          selecting={selecting}
          onToggleSelecting={() => {
            setSelecting((value) => !value);
            setSelected(new Set());
          }}
          actions={treeActions}
          onSelectionChange={setSelected}
        />
      ) : (
        <TrashList
          items={trashedFiles}
          selectedPath={trashFlow.trashSelected}
          checked={trashFlow.trashChecked}
          selecting={trashFlow.selecting}
          onToggleSelecting={trashFlow.toggleSelecting}
          onSelectionChange={trashFlow.setTrashChecked}
          onSelect={(path) => {
            trashFlow.setTrashSelected(path);
            openProperties(path, true);
          }}
          onToggleSelect={(path) =>
            trashFlow.setTrashChecked((prev) => {
              const next = new Set(prev);
              if (next.has(path)) next.delete(path);
              else next.add(path);
              return next;
            })
          }
          onRestore={(paths) =>
            void trashFlow.batchRestore(paths)
          }
          onRestoreAll={() => void trashFlow.batchRestore()}
          onPurgeChecked={(paths) => setDialog({ type: "purge", paths })}
          restoringAll={trashFlow.restoringAll}
          purging={trashFlow.purging}
        />
      )}
    </Sidebar>
  );

  const mainPane =
    mode === "workspace" ? (
      <div className="h-full min-h-0">
        <EditorPane
          state={editor}
          onChange={changeEditor}
          onStateChange={changeEditor}
          onOpenProperties={() => openProperties(activeEditorPath)}
          onOpenReference={requestOpen}
          onOpenTag={openTag}
          completions={completionFiles}
          onAuthLost={() => onLogout(t("bannerAuthLost"))}
          canTrash={canTrash}
          onTrashCurrent={() => {
            const current = editorRef.current;
            if (current.kind === "clean" || current.kind === "dirty") {
              setDialog({ type: "trash", paths: [current.path] });
            }
          }}
        />
      </div>
    ) : (
      <TrashViewer
        context={{ files: completionFiles, onOpen: requestOpen, onTag: openTag }}
        item={trashedFiles.find((one) => one.path === trashFlow.trashSelected) ?? null}
        onRestore={(path) => void trashFlow.restoreFromTrash(path)}
        onCopyPath={treeActions.onCopyPath}
        onPurge={(path) => setDialog({ type: "purge", paths: [path] })}
        purging={trashFlow.purging}
        restoring={trashFlow.restoringAll}
      />
    );

  const propertiesPane = (
    <PropertiesPanel
      files={completionFiles}
      onOpenTag={openTag}
      onOpenFile={requestOpen}
      path={propertiesPath}
      entry={
        liveFiles.find(file => file.path === propertiesPath)
          ?? trashedFiles.find(file => file.path === propertiesPath)
      }
      revision={
        mode === "workspace" && propertiesPath && propertiesPath === activeEditorPath
          ? (editor.kind !== "empty" && editor.kind !== "loading" && editor.kind !== "conflict"
            ? editor.baseRevision
            : null)
          : null
      }
      onRestore={(revision) => {
        if (mode === "trash" && propertiesPath) {
          trashFlow.restoreTrashedRevision(propertiesPath, revision);
        } else if (propertiesPath && propertiesPath === activeEditorPath && editor.kind !== "empty" && editor.kind !== "loading" && editor.kind !== "conflict") {
          restoreRevision(propertiesPath, revision, editor.baseRevision);
        } else if (propertiesPath) {
          // Properties of a non-open file: restore against its HEAD.
          void (async () => {
            try {
              const meta = await api.readMeta(propertiesPath);
              await api.restore(propertiesPath, revision, meta.revision);
              toast.success(t("bannerRestoredShort"));
              await refreshFiles();
            } catch (error) {
              fail(error);
            }
          })();
        }
      }}
      onClose={dismissProperties}
    />
  );

  return (
    <div className="flex h-full min-h-0 flex-col bg-background" {...overlayHandlers}>
      <TopBar
        session={session}
        navigation={navigation}
        sidebarCollapsed={sidebarCollapsed}
        onToggleSidebar={toggleSidebar}
        onOpenPalette={() => setPaletteOpen(true)}
        onNewDocument={() => treeActions.onNewDocumentIn("")}
        onUpload={openUploadIntent}
        onOpenSettings={() => setSettingsOpen(true)}
        onLogout={onLogout}
      />

      {IS_DEMO && (
        <p className="shrink-0 border-b bg-muted px-3 py-1.5 text-center text-xs text-muted-foreground" data-testid="demo-notice">
          {t("demoNotice")}
        </p>
      )}

      {isMobile ? (
        <div className="min-h-0 flex-1">
          <MobileLayout
            tab={mobileTab}
            onTabChange={setMobileTab}
            labels={{
              files: t("mobileTabFiles"),
              editor: t("mobileTabEditor"),
              info: t("mobileTabInfo"),
            }}
            name={t("mobileTabs")}
            files={sidebarPane}
            editor={mainPane}
            info={propertiesPane}
          />
        </div>
      ) : (
        <Group orientation="horizontal" className="min-h-0 flex-1">
          <Panel
            panelRef={sidebarPanel}
            collapsible
            collapsedSize="0"
            defaultSize="20"
            minSize="16"
            maxSize="32"
          >
            {sidebarPane}
          </Panel>

          <Separator className="w-px bg-border transition-colors hover:bg-primary/40 data-[state=dragging]:bg-primary/60" />

          <Panel minSize="35">{mainPane}</Panel>

          {propertiesOpen && (
            <>
              <Separator className="w-px bg-border transition-colors hover:bg-primary/40 data-[state=dragging]:bg-primary/60" />
              <Panel defaultSize="24" minSize="16" maxSize="40">
                {propertiesPane}
              </Panel>
            </>
          )}
        </Group>
      )}

      {dragging && mode === "workspace" && (
        <div className="pointer-events-none fixed inset-0 z-30 flex items-center justify-center bg-primary/4">
          <div className="rounded-[12px] border border-dashed border-primary/50 px-8 py-6 text-body font-medium text-primary">
            {t("dropHere")}
          </div>
        </div>
      )}

      {banner && <Banner message={banner} onClose={() => setBanner(null)} />}

      <SettingsDialog open={settingsOpen} onOpenChange={setSettingsOpen} />

      <CommandPalette
        open={paletteOpen}
        tags={paletteTags}
        onTagsChange={setPaletteTags}
        onOpenChange={(open) => {
          setPaletteOpen(open);
          if (!open) setPaletteTags([]);
        }}
        files={liveFiles}
        searchMode={searchMode}
        onSearchModeChange={setSearchMode}
        onOpenFile={requestOpen}
        onNewDocument={() => treeActions.onNewDocumentIn("")}
        onUpload={openUploadIntent}
      />

      {upload.conflict && (
        <UploadConflictDialog
          plan={upload.conflict.plan}
          existing={upload.conflict.existing}
          running={upload.running}
          onCancel={upload.cancel}
          onConfirm={(plan) => void upload.confirm(plan).then(runUpload)}
        />
      )}
      {upload.progress && (
        <UploadProgress items={upload.progress} onDismiss={upload.dismissProgress} />
      )}

      <UploadInputs
        fileRef={fileInputRef}
        folderRef={folderInputRef}
        onFiles={(files, uploadMode) => {
          if (files.length > 0) void upload.begin(files, uploadMode).then(runUpload);
        }}
      />

      <WorkspaceDialogs
        dialog={dialog}
        onClose={() => { navigation.cancel(); setDialog(null); }}
        folders={folders}
        batchRunning={batchRunning}
        purging={trashFlow.purging}
        onConfirmNewFile={(path) => newFile(path)}
        onConfirmNewFolder={(path) => createFolder(path)}
        onConfirmTrash={(paths) => void trashFlow.trashPaths(paths)}
        onConfirmPurge={(paths) => void trashFlow.purgePaths(paths)}
        onConfirmDiscardOpen={() => {
          const current = editorRef.current;
          if (current.kind !== "empty") clearDraft(current.path);
          setDialog(null);
          navigation.confirm();
        }}
        onConfirmMove={confirmMove}
      />
    </div>
  );
}
