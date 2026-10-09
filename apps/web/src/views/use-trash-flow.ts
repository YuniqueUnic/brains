import { useEffect, useState } from "react";
import { toast } from "sonner";
import { api, type Entry } from "../api/client";
import { newestContentful } from "../components/trash/content";

/*
 * Trash flow (design §8): every mutation on tombstones — batch trash,
 * restore (single / batch / exact revision) and permanent purge — refreshes
 * the listing afterwards and prunes the trash UI state that referenced
 * vanished entries. The trash selection/checks live here too: they are the
 * flow's own bookkeeping, not workspace layout state.
 */
export function useTrashFlow({
  trashedFiles,
  refreshFiles,
  fail,
  setBanner,
  onTrashed,
  closeDialog,
  setBatchRunning,
  entryRevision,
  t,
}: {
  trashedFiles: Entry[];
  refreshFiles: () => Promise<void>;
  fail: (error: unknown) => void;
  setBanner: (message: string | null) => void;
  /** Paths just trashed; the workspace clears selection and drops its open editor if hit. */
  onTrashed: (paths: string[]) => void;
  closeDialog: () => void;
  setBatchRunning: (running: boolean) => void;
  /** Live-file revision lookup: the open editor's baseRevision wins. */
  entryRevision: (path: string) => string | undefined;
  t: (key: string, vars?: Record<string, unknown>) => string;
}) {
  const [trashSelected, setTrashSelected] = useState<string | null>(null);
  const [trashChecked, setTrashChecked] = useState<Set<string>>(new Set());
  const [restoringAll, setRestoringAll] = useState(false);
  const [purging, setPurging] = useState(false);

  // A restored/purged (or externally vanished) tombstone leaves the trash
  // list — both the viewer selection and the bulk checks go with it.
  useEffect(() => {
    if (trashSelected && !trashedFiles.some((item) => item.path === trashSelected)) {
      setTrashSelected(null);
    }
    setTrashChecked((prev) => {
      if (prev.size === 0) return prev;
      const next = new Set([...prev].filter((path) => trashedFiles.some((item) => item.path === path)));
      return next.size === prev.size ? prev : next;
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [trashedFiles]);

  const restoreOne = async (path: string): Promise<boolean> => {
    // The tombstone itself has no content: restore the newest contentful
    // revision reachable from it (design §8.1: restore takes a live source).
    const found = await newestContentful(path);
    if (!found) return false;
    await api.restore(path, found.revision, found.anchor);
    return true;
  };

  const trashPaths = async (paths: string[]) => {
    closeDialog();
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
    try {
      await refreshFiles();
    } catch (error) {
      fail(error);
    }
    // Only succeeded paths: a failed trash (stale revision) must keep both
    // the file and any editor buffer bound to it.
    onTrashed(paths.filter((path) => !failed.includes(path)));
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
      // Success confirmation rides the toast channel (see Workspace.restoreRevision).
      toast.success(t("bannerRestored", { path }));
      await refreshFiles();
    } catch (error) {
      fail(error);
    }
  };

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

  // Permanent delete (owner decision 2026-10): tombstone revision from the
  // listing is the precondition; the server refuses live paths (409).
  const purgePaths = async (paths: string[]) => {
    closeDialog();
    setPurging(true);
    const failed: string[] = [];
    let done = 0;
    for (const path of paths) {
      try {
        const revision = trashedFiles.find((item) => item.path === path)?.revision;
        if (!revision) throw new Error("missing tombstone revision");
        await api.purge(path, revision);
        done += 1;
      } catch {
        failed.push(path);
      }
    }
    setPurging(false);
    setTrashChecked(new Set());
    try {
      await refreshFiles();
    } catch (error) {
      fail(error);
    }
    if (failed.length > 0) {
      toast.warning(t("batchPartial", { done, failed: failed.length, detail: failed.slice(0, 3).join("、") }));
    } else {
      toast.success(t("purgeDone", { count: done }));
    }
  };

  return {
    trashSelected,
    setTrashSelected,
    trashChecked,
    setTrashChecked,
    restoringAll,
    purging,
    trashPaths,
    restoreTrashedRevision,
    restoreFromTrash,
    batchRestore,
    purgePaths,
  };
}
