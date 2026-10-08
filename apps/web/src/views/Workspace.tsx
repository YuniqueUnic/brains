import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Group, Panel, Separator, type PanelImperativeHandle } from "react-resizable-panels";
import {
  api,
  ApiError,
  isTextMedia,
  type Entry,
  type SessionInfo,
} from "../api/client";
import { TopBar, type UploadIntent } from "../components/shell/top-bar";
import { SettingsDialog } from "../components/shell/settings-dialog";
import { Sidebar, type WorkspaceMode } from "../components/shell/sidebar";
import { FileTree, collectFolders, type TreeActions } from "../components/tree/file-tree";
import { CommandPalette } from "../components/search/command-palette";
import EditorPane, { type EditorState } from "../components/EditorPane";
import { PropertiesPanel } from "../components/properties/properties-panel";
import { TrashList } from "../components/trash/trash-list";
import { TrashViewer } from "../components/trash/trash-viewer";
import { newestContentful } from "../components/trash/content";
import {
  UploadConflictDialog,
} from "../components/upload/upload-conflict-dialog";
import { UploadProgress } from "../components/upload/upload-progress";
import { useUpload, type UploadSummary } from "../components/upload/use-upload";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogTitle,
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
import { TriangleAlert, X, FolderInput } from "lucide-react";

/*
 * Workspace orchestrator (DESIGN.md §61-62): explicit state unions instead of
 * boolean soup — mode (workspace/trash), dialog (discriminated), selection
 * (Set), the on-demand properties panel and the ⌘K palette. All business
 * mutations flow through the conditional-write API client.
 */

type SidePanel =
  | { type: "none" }
  // follow: the panel tracks the context selection (editor file in the
  // workspace, trash selection in the trash) instead of a pinned path.
  | { type: "properties"; path: string | null; follow: boolean };

type DialogState =
  | null
  | { type: "new-document"; folder: string }
  | { type: "new-folder"; folder: string }
  | { type: "trash"; paths: string[] }
  | { type: "discard-open"; path: string }
  | { type: "move"; targets: string[] };

export default function Workspace({
  session,
  onLogout,
}: {
  session: SessionInfo;
  onLogout: () => void;
}) {
  const { t } = useTranslation("workspace");
  const { t: tCommon } = useTranslation("common");
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
  const closeProperties = () => setPanel({ type: "none" });
  const [trashSelected, setTrashSelected] = useState<string | null>(null);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [searchMode, setSearchMode] = useState<"literal" | "fts">("literal");
  const [dialog, setDialog] = useState<DialogState>(null);
  const [selecting, setSelecting] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [newFilePath, setNewFilePath] = useState("");
  const [moveDestination, setMoveDestination] = useState("");
  const [batchRunning, setBatchRunning] = useState(false);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [dragging, setDragging] = useState(false);
  const sidebarPanel = useRef<PanelImperativeHandle | null>(null);
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const folderInputRef = useRef<HTMLInputElement | null>(null);
  const editorRef = useRef(editor);
  editorRef.current = editor;
  const [banner, setBanner] = useState<string | null>(null);

  // A restored (or externally vanished) tombstone leaves the trash list —
  // the selection goes with it.
  useEffect(() => {
    if (trashSelected && !trashedFiles.some((item) => item.path === trashSelected)) {
      setTrashSelected(null);
    }
  }, [trashedFiles, trashSelected]);

  const folders = useMemo(() => collectFolders(liveFiles), [liveFiles]);
  const folderSet = useMemo(() => new Set(folders), [folders]);
  // Reference completions for the editor ([[wiki]], [](/![]() links).
  const completionFiles = useMemo(
    () =>
      liveFiles.map((entry) => ({
        path: entry.path,
        binary: !isTextMedia(entry.media_type),
      })),
    [liveFiles],
  );
  const shownFiles = mode === "workspace" ? liveFiles : trashedFiles;

  const fail = (error: unknown) => {
    // An expired session has no in-page recovery: back to login.
    if (error instanceof ApiError && error.status === 401) {
      setBanner(t("bannerAuthLost"));
      onLogout();
      return;
    }
    setBanner(error instanceof Error ? error.message : String(error));
  };

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
  };

  useEffect(() => {
    void refreshFiles().catch(fail);
    const clearDrag = () => setDragging(false);
    window.addEventListener("dragend", clearDrag);
    window.addEventListener("drop", clearDrag);
    return () => {
      window.removeEventListener("dragend", clearDrag);
      window.removeEventListener("drop", clearDrag);
    };
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

  const runUpload = async (summary: UploadSummary | undefined) => {
    if (!summary) return;
    if (summary.failed.length > 0) {
      toast.warning(
        t("uploadPartial", {
          done: summary.done,
          failed: summary.failed.length,
          detail: summary.failed.slice(0, 3).join("、"),
        }),
      );
    } else {
      const skipped = summary.skipped + summary.identical;
      toast.success(
        skipped > 0
          ? t("uploadDoneToastSkipped", { done: summary.done, skipped })
          : t("uploadDoneToast", { done: summary.done }),
      );
    }
  };

  // Clipboard intake (paste anywhere, or the 上传 menu item): images land as
  // unnamed.png (+N), text as unnamed.md. The names are synthetic, so the
  // rename plan lands silently — real files picked or dropped keep the
  // conflict dialog.
  const importFromClipboard = async () => {
    try {
      const items = await navigator.clipboard.read();
      const files: File[] = [];
      for (const item of items) {
        const imageType = item.types.find((candidate) => candidate.startsWith("image/"));
        if (imageType) {
          const blob = await item.getType(imageType);
          const ext = imageType.split("/")[1]?.replace(/[^a-z0-9]/gi, "") || "png";
          files.push(new File([blob], `unnamed.${ext}`, { type: imageType }));
          continue;
        }
        if (item.types.includes("text/plain")) {
          const text = await (await item.getType("text/plain")).text();
          if (text.trim()) {
            files.push(new File([text], "unnamed.md", { type: "text/markdown" }));
          }
        }
      }
      if (files.length === 0) {
        toast.info(t("clipboardEmpty"));
        return;
      }
      runUpload(await upload.begin(files, "flat", { autoConfirm: true }));
    } catch {
      toast.error(t("clipboardUnavailable"));
    }
  };

  // Paste anywhere outside a text field imports the clipboard: files via the
  // normal conflict pipeline, bare text as a new unnamed.md note.
  const importRef = useRef<(files: File[], autoConfirm: boolean) => void>(() => undefined);
  importRef.current = (files, autoConfirm) => {
    void upload.begin(files, "flat", { autoConfirm }).then(runUpload);
  };
  const modeRef = useRef(mode);
  modeRef.current = mode;
  useEffect(() => {
    const onPaste = (event: ClipboardEvent) => {
      if (modeRef.current !== "workspace") return;
      const target = event.target as HTMLElement | null;
      if (target?.closest("input, textarea, [contenteditable=true], .cm-content")) return;
      const files = Array.from(event.clipboardData?.files ?? []);
      if (files.length > 0) {
        event.preventDefault();
        importRef.current(files, false);
        return;
      }
      const text = event.clipboardData?.getData("text/plain") ?? "";
      if (text.trim()) {
        event.preventDefault();
        importRef.current([new File([text], "unnamed.md", { type: "text/markdown" })], true);
      }
    };
    document.addEventListener("paste", onPaste);
    return () => document.removeEventListener("paste", onPaste);
  }, []);

  const openUploadIntent = (intent: UploadIntent) => {
    if (intent === "files") fileInputRef.current?.click();
    else if (intent === "folder") folderInputRef.current?.click();
    else void importFromClipboard();
  };

  // ---- Open / guard ------------------------------------------------------

  const openRunRef = useRef(0);
  const openFile = async (path: string) => {
    const runId = openRunRef.current + 1;
    openRunRef.current = runId;
    const stale = () => openRunRef.current !== runId;
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
  };

  const requestOpen = (path: string) => {
    const current = editorRef.current;
    // `saving` counts too: keystrokes typed during the flight have not been
    // sent yet and would be dropped without a word.
    if (
      current.kind === "dirty" ||
      current.kind === "conflict" ||
      current.kind === "error" ||
      current.kind === "saving"
    ) {
      setDialog({ type: "discard-open", path });
      return;
    }
    void openFile(path);
  };

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

  // A batch target is either a file or a folder path (with or without the
  // trailing slash the context menus pass): folders expand to every file
  // beneath them so tombstone calls always address real entries.
  const expandEntry = (raw: string): string[] => {
    const path = raw.replace(/\/+$/, "");
    const self = liveFiles.some((entry) => entry.path === path) ? [path] : [];
    if (!folderSet.has(path)) return self.length > 0 ? self : [raw];
    return [
      ...self,
      ...liveFiles.filter((entry) => entry.path.startsWith(`${path}/`)).map((entry) => entry.path),
    ];
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
      const expanded = [...new Set(paths.flatMap(expandEntry))];
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
        const taken = new Set(liveFiles.map((entry) => entry.path));
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
      const url = `${window.location.origin}/api/v1/file?path=${encodeURIComponent(path)}`;
      void navigator.clipboard.writeText(url).then(() => toast.success(t("copyNetPathDone")));
    },
    onNewDocumentIn: (folder) => {
      setNewFilePath(folder ? `${folder}/` : "");
      setDialog({ type: "new-document", folder });
    },
    onCreateFolderIn: (folder) => {
      setNewFilePath(folder ? `${folder}/` : "");
      setDialog({ type: "new-folder", folder });
    },
    onDropMove: (entries, into) => moveBatch(entries, into),
    onProperties: (path) => openProperties(path),
    onMove: (targets) => {
      setMoveDestination("");
      setDialog({ type: "move", targets });
    },
  };

  // One mover for drag&drop and the move dialog: folders travel as folders
  // (children keep their relative structure), files travel one by one. The
  // editor reopens the moved document only after the listing refreshes.
  const moveBatch = (entries: { path: string; isFolder: boolean }[], into: string) => {
    // Moving a folder into itself or a descendant would nest it inside its
    // own content — refused up front, like every OS file manager.
    const movable = entries.filter((entry) => {
      if (!entry.isFolder) return true;
      const plain = entry.path.replace(/\/+$/, "");
      return into !== plain && !into.startsWith(`${plain}/`);
    });
    const refused = entries.length - movable.length;
    if (movable.length === 0) {
      if (refused > 0) toast.warning(t("moveIntoSelf"));
      return;
    }
    void (async () => {
      setBatchRunning(true);
      setSelected(new Set());
      const failed: string[] = [];
      let done = 0;
      let movedOpenTarget: string | null = null;
      for (const entry of movable) {
        if (entry.isFolder) {
          const prefix = entry.path.endsWith("/") ? entry.path : `${entry.path}/`;
          const name = prefix.slice(0, -1).split("/").pop() ?? prefix;
          const destBase = into ? `${into}/${name}` : name;
          const moves: { source: string; target: string }[] = [];
          for (const file of liveFiles.filter((one) => one.path.startsWith(prefix))) {
            const target = `${destBase}/${file.path.slice(prefix.length)}`;
            if (target !== file.path) moves.push({ source: file.path, target });
          }
          for (const move of moves) {
            const revision = entryRevision(move.source);
            if (!revision) {
              failed.push(move.source);
              continue;
            }
            try {
              const result = await api.move(move.source, move.target, revision);
              if (result.error || result.partial) {
                failed.push(move.source);
                continue;
              }
              done += 1;
              const current = editorRef.current;
              if (current.kind !== "empty" && current.kind !== "loading" && current.path === move.source) {
                movedOpenTarget = move.target;
              }
            } catch {
              failed.push(move.source);
            }
          }
        } else {
          const name = entry.path.split("/").pop() ?? entry.path;
          const target = into ? `${into}/${name}` : name;
          if (target === entry.path) continue;
          const revision = entryRevision(entry.path);
          if (!revision) {
            failed.push(entry.path);
            continue;
          }
          try {
            const result = await api.move(entry.path, target, revision);
            if (result.error || result.partial) {
              failed.push(entry.path);
              continue;
            }
            done += 1;
            const current = editorRef.current;
            if (current.kind !== "empty" && current.kind !== "loading" && current.path === entry.path) {
              movedOpenTarget = target;
            }
          } catch {
            failed.push(entry.path);
          }
        }
      }
      setBatchRunning(false);
      try {
        await refreshFiles();
      } catch (error) {
        fail(error);
      }
      if (movedOpenTarget) requestOpen(movedOpenTarget);
      if (failed.length > 0) {
        toast.warning(
          t("batchPartial", {
            done,
            failed: failed.length,
            detail: failed.slice(0, 3).join("、"),
          }),
        );
      } else if (done > 0 || refused > 0) {
        if (refused > 0) toast.warning(t("moveIntoSelf"));
        if (done > 0) {
          toast.success(t("moveDone", { count: done, destination: into === "" ? "/" : into }));
        }
      }
    })();
  };

  // ---- Trash flow ----------------------------------------------------------

  const trashPaths = async (paths: string[]) => {
    setDialog(null);
    setBatchRunning(true);
    const failed: string[] = [];
    let done = 0;
    for (const path of paths) {
      try {
        const revision = entryRevision(path);
        if (!revision) throw new Error("missing revision");
        await api.trash(path, revision);
        done += 1;
      } catch {
        failed.push(path);
      }
    }
    setBatchRunning(false);
    setSelected(new Set());
    try {
      await refreshFiles();
    } catch (error) {
      fail(error);
    }
    if (editorRef.current.kind !== "empty" && editorRef.current.kind !== "loading" && paths.includes(editorRef.current.path)) {
      setEditor({ kind: "empty" });
    }
    if (failed.length > 0) {
      toast.warning(
        t("batchPartial", {
          done,
          failed: failed.length,
          detail: failed.slice(0, 3).join("、"),
        }),
      );
    } else {
      toast.success(t("batchTrashDone", { count: done }));
    }
  };

  const restoreOne = async (path: string): Promise<boolean> => {
    // The tombstone itself has no content: restore the newest contentful
    // revision reachable from it (design §8.1: restore takes a live source).
    const found = await newestContentful(path);
    if (!found) return false;
    await api.restore(path, found.revision, found.anchor);
    return true;
  };

  // Restore from the properties panel while in the trash: base is the
  // tombstone's anchor (the editor-based path only applies to live files).
  const restoreTrashedRevision = (path: string, revision: string) => {
    void (async () => {
      try {
        const anchor = (await api.history(path)).anchor_revision;
        await api.restore(path, revision, anchor);
        toast.success(t("bannerRestoredShort"));
        await refreshFiles();
      } catch (error) {
        fail(error);
      }
    })();
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

  const [restoringAll, setRestoringAll] = useState(false);
  const batchRestore = async () => {
    if (restoringAll || trashedFiles.length === 0) return;
    setRestoringAll(true);
    const failed: string[] = [];
    let done = 0;
    for (const item of trashedFiles) {
      try {
        if (await restoreOne(item.path)) done += 1;
        else failed.push(item.path);
      } catch {
        failed.push(item.path);
      }
    }
    setRestoringAll(false);
    try {
      await refreshFiles();
    } catch (error) {
      fail(error);
    }
    if (failed.length > 0) {
      toast.warning(t("batchPartial", { done, failed: failed.length, detail: failed.slice(0, 3).join("、") }));
    } else {
      toast.success(t("batchRestoreDone", { count: done }));
    }
  };

  // ---- New document / move confirm ---------------------------------------

  const newFile = async () => {
    const path = newFilePath.trim();
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
      setNewFilePath("");
      await refreshFiles();
      setEditor({ kind: "clean", path: result.path, text: "", baseText: "", baseRevision: result.revision });
    } catch (error) {
      fail(error);
    }
  };

  const createFolder = async () => {
    const raw = newFilePath.trim().replace(/\/+/g, "/").replace(/\/+$/, "");
    if (!raw) return;
    try {
      await api.saveText(`${raw}/.keep`, "", { mode: "create" });
      setDialog(null);
      setNewFilePath("");
      await refreshFiles();
      toast.success(t("folderCreated", { path: raw }));
    } catch (error) {
      fail(error);
    }
  };

  const moveEntryCount = dialog?.type === "move" ? dialog.targets.length : 0;
  const confirmMove = () => {
    const destination = moveDestination.trim().replace(/\/+$/, "");
    if (!destination || moveEntryCount === 0) return;
    setDialog(null);
    // Dialog targets are file paths or "folder/" entries; anything that is a
    // known folder prefix moves as a folder so children keep their structure.
    const entries = (dialog?.type === "move" ? dialog.targets : []).map((raw) => ({
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
  const contextPath = mode === "workspace" ? activeEditorPath : trashSelected;
  const propertiesPath =
    panel.type === "properties"
      ? panel.follow
        ? contextPath
        : panel.path
      : null;

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
        setBanner(t("bannerRestoredShort"));
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
      if (meta && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setPaletteOpen((open) => !open);
      } else if (meta && event.key.toLowerCase() === "i") {
        event.preventDefault();
        if (propertiesOpen && propertiesPath === activeEditorPath) setPanel({ type: "none" });
        else openProperties(activeEditorPath, true);
      } else if (meta && event.key.toLowerCase() === "n") {
        event.preventDefault();
        setNewFilePath("");
        setDialog({ type: "new-document", folder: "" });
      } else if (
        (event.key === "Delete" || event.key === "Backspace") &&
        selected.size > 0
      ) {
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
      } else if (event.key === "Escape" && !event.defaultPrevented) {
        // Esc belongs to the layer that consumed it first (find panel,
        // palette, menus all preventDefault); only a "free" Esc closes the
        // properties panel and clears the batch selection.
        const target = event.target as HTMLElement | null;
        const inChrome =
          target?.isContentEditable ||
          target?.closest("input, textarea, .cm-content, .cm-panel") != null;
        if (!inChrome) {
          setPanel({ type: "none" });
          setSelected(new Set());
        }
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [activeEditorPath, propertiesOpen, propertiesPath, selected, treeActions]);

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

  return (
    <div
      className="flex h-full min-h-0 flex-col bg-background"
      onDragOver={(event) => {
        // Only external file drags show the import overlay; the tree's own
        // move-drag carries a custom payload and must not trigger it.
        if (mode !== "workspace") return;
        if (!event.dataTransfer.types.includes("Files")) return;
        event.preventDefault();
        setDragging(true);
      }}
      onDragLeave={(event) => {
        if (
          !event.relatedTarget ||
          !(event.currentTarget as HTMLElement).contains(event.relatedTarget as Node)
        ) {
          setDragging(false);
        }
      }}
      onDrop={(event) => {
        setDragging(false);
        if (mode !== "workspace") return;
        if (!event.dataTransfer.types.includes("Files")) return;
        event.preventDefault();
        const transfer = event.dataTransfer;
        void upload.begin(transfer.items, "tree").then((summary) => {
          // Some drop sources expose plain files but no entry objects —
          // fall back to the flat file list before giving up.
          if (summary === undefined && transfer.files.length > 0) {
            return upload.begin(Array.from(transfer.files), "tree");
          }
          return summary;
        }).then(runUpload);
      }}
    >
      <TopBar
        session={session}
        sidebarCollapsed={sidebarCollapsed}
        onToggleSidebar={toggleSidebar}
        onOpenPalette={() => setPaletteOpen(true)}
        onNewDocument={() => treeActions.onNewDocumentIn("")}
        onUpload={openUploadIntent}
        onOpenSettings={() => setSettingsOpen(true)}
        onLogout={onLogout}
      />

      <Group orientation="horizontal" className="min-h-0 flex-1">
        <Panel
          panelRef={sidebarPanel}
          collapsible
          collapsedSize="0"
          defaultSize="20"
          minSize="16"
          maxSize="32"
        >
          <Sidebar
            mode={mode}
            onModeChange={(next) => {
              setMode(next);
              setSelected(new Set());
              setRenaming(null);
              setTrashSelected(null);
            }}
            fileCount={shownFiles.length}
            truncated={filesTruncated}
          >
            {mode === "workspace" ? (
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
                selectedPath={trashSelected}
                onSelect={(path) => {
                  setTrashSelected(path);
                  openProperties(path, true);
                }}
                onRestore={(path) => void restoreFromTrash(path)}
                onRestoreAll={() => void batchRestore()}
                restoringAll={restoringAll}
              />
            )}
          </Sidebar>
        </Panel>

        <Separator className="w-px bg-border transition-colors hover:bg-primary/40 data-[state=dragging]:bg-primary/60" />

        <Panel minSize="35">
          {mode === "workspace" ? (
            <div className="h-full min-h-0">
              <EditorPane
                state={editor}
                onChange={setEditor}
                onStateChange={setEditor}
                onOpenProperties={() => openProperties(activeEditorPath)}
                onOpenReference={requestOpen}
                completions={completionFiles}
                onAuthLost={() => {
                  setBanner(t("bannerAuthLost"));
                  onLogout();
                }}
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
              item={trashedFiles.find((one) => one.path === trashSelected) ?? null}
              onRestore={(path) => void restoreFromTrash(path)}
              onCopyPath={treeActions.onCopyPath}
            />
          )}
        </Panel>

        {propertiesOpen && (
          <>
            <Separator className="w-px bg-border transition-colors hover:bg-primary/40 data-[state=dragging]:bg-primary/60" />
            <Panel defaultSize="24" minSize="16" maxSize="40">
              <PropertiesPanel
                onOpenFile={requestOpen}
                path={propertiesPath}
                revision={
                  mode === "workspace" && propertiesPath && propertiesPath === activeEditorPath
                    ? (editor.kind !== "empty" && editor.kind !== "loading" && editor.kind !== "conflict"
                      ? editor.baseRevision
                      : null)
                    : null
                }
                onRestore={(revision) => {
                  if (mode === "trash" && propertiesPath) {
                    restoreTrashedRevision(propertiesPath, revision);
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
                onClose={closeProperties}
              />
            </Panel>
          </>
        )}
      </Group>

      {dragging && mode === "workspace" && (
        <div className="pointer-events-none fixed inset-0 z-30 flex items-center justify-center bg-primary/4">
          <div className="rounded-[12px] border border-dashed border-primary/50 px-8 py-6 text-[13px] font-medium text-primary">
            {t("dropHere")}
          </div>
        </div>
      )}

      {banner && (
        <div className="pointer-events-none fixed inset-x-0 top-14 z-40 flex justify-center px-4">
          <div
            className="pointer-events-auto flex max-w-xl items-start gap-2 rounded-[8px] border bg-card px-3 py-2 text-xs shadow-overlay"
            data-testid="banner"
            role="status"
          >
            <TriangleAlert className="mt-0.5 size-3.5 shrink-0 stroke-[1.5] text-warning" />
            <span className="min-w-0 flex-1 leading-relaxed">{banner}</span>
            <button
              onClick={() => setBanner(null)}
              aria-label={tCommon("close")}
              className="cursor-pointer rounded p-0.5 text-muted-foreground hover:bg-muted hover:text-foreground"
            >
              <X className="size-3.5 stroke-[1.5]" />
            </button>
          </div>
        </div>
      )}

      <SettingsDialog open={settingsOpen} onOpenChange={setSettingsOpen} />

      <CommandPalette
        open={paletteOpen}
        onOpenChange={setPaletteOpen}
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

      <input
        ref={fileInputRef}
        type="file"
        multiple
        hidden
        onChange={(event) => {
          const files = Array.from(event.target.files ?? []);
          if (files.length > 0) void upload.begin(files, "flat").then(runUpload);
          event.target.value = "";
        }}
      />
      <input
        ref={folderInputRef}
        type="file"
        multiple
        hidden
        {...({ webkitdirectory: "true", directory: "true" } as Record<string, string>)}
        onChange={(event) => {
          const files = Array.from(event.target.files ?? []);
          if (files.length > 0) void upload.begin(files, "tree").then(runUpload);
          event.target.value = "";
        }}
      />

      {/* ---- Dialogs ---- */}

      <Dialog
        open={dialog?.type === "new-document" || dialog?.type === "new-folder"}
        onOpenChange={(open) => !open && setDialog(null)}
      >
        <DialogContent data-testid="new-file-dialog">
          <DialogHeader>
            <DialogTitle>
              {dialog?.type === "new-folder" ? t("newFolderDialogTitle") : t("newFileDialogTitle")}
            </DialogTitle>
            <DialogDescription>
              {dialog?.type === "new-folder" ? t("newFolderDialogHint") : t("newFileDialogHint")}
            </DialogDescription>
          </DialogHeader>
          <div className="pt-4">
            <Input
              data-testid="new-file-path"
              placeholder={dialog?.type === "new-folder" ? t("newFolderPlaceholder") : t("newFilePlaceholder")}
              value={newFilePath}
              onChange={(event) => setNewFilePath(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  void (dialog?.type === "new-folder" ? createFolder() : newFile());
                }
              }}
              autoFocus
            />
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDialog(null)}>
              {tCommon("cancel")}
            </Button>
            <Button
              variant="default"
              data-testid="new-file-confirm"
              onClick={() => void (dialog?.type === "new-folder" ? createFolder() : newFile())}
            >
              {tCommon("create")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <AlertDialog
        open={dialog?.type === "trash"}
        onOpenChange={(open) => !open && setDialog(null)}
      >
        <AlertDialogContent data-testid="trash-confirm">
          <AlertDialogTitle>{t("trashConfirmTitle")}</AlertDialogTitle>
          <AlertDialogDescription>
            {dialog?.type === "trash" ? t("trashConfirmBody", { count: dialog.paths.length }) : ""}
          </AlertDialogDescription>
          <AlertDialogFooter>
            <AlertDialogCancel>{tCommon("cancel")}</AlertDialogCancel>
            <AlertDialogAction
              data-testid="trash-confirm-yes"
              disabled={batchRunning}
              onClick={() => {
                if (dialog?.type === "trash") void trashPaths(dialog.paths);
              }}
            >
              {t("trashConfirmAction")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog
        open={dialog?.type === "discard-open"}
        onOpenChange={(open) => !open && setDialog(null)}
      >
        <AlertDialogContent data-testid="open-confirm">
          <AlertDialogTitle>{t("openConfirmTitle")}</AlertDialogTitle>
          <AlertDialogDescription>
            {dialog?.type === "discard-open"
              ? t("openConfirmBody", { path: dialog.path })
              : ""}
          </AlertDialogDescription>
          <AlertDialogFooter>
            <AlertDialogCancel>{tCommon("cancel")}</AlertDialogCancel>
            <AlertDialogAction
              data-testid="open-confirm-yes"
              onClick={() => {
                const target = dialog?.type === "discard-open" ? dialog.path : null;
                setDialog(null);
                if (target) void openFile(target);
              }}
            >
              {t("openConfirmAction")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <Dialog
        open={dialog?.type === "move"}
        onOpenChange={(open) => !open && setDialog(null)}
      >
        <DialogContent data-testid="move-dialog">
          <DialogHeader>
            <DialogTitle>{t("moveTitle")}</DialogTitle>
            <DialogDescription>
              {t("moveDescription", { count: moveEntryCount })}
            </DialogDescription>
          </DialogHeader>
          <div className="flex flex-col gap-1.5 pt-4">
            <Input
              data-testid="move-destination"
              placeholder={t("movePlaceholder")}
              value={moveDestination}
              onChange={(event) => setMoveDestination(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") void confirmMove();
              }}
              autoFocus
            />
            <div className="flex max-h-40 flex-col gap-0.5 overflow-y-auto">
              {folders.map((folder) => (
                <button
                  key={folder}
                  type="button"
                  onClick={() => setMoveDestination(folder)}
                  className="flex h-7 cursor-pointer items-center gap-2 rounded-md px-2 text-left font-mono text-[11.5px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                >
                  <FolderInput className="size-3.5 shrink-0 stroke-[1.5]" />
                  {folder === "" ? t("conflictRoot") : `${folder}/`}
                </button>
              ))}
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDialog(null)}>
              {tCommon("cancel")}
            </Button>
            <Button variant="default" disabled={batchRunning} onClick={() => void confirmMove()}>
              {t("moveAction")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
