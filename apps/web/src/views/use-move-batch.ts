import { toast } from "sonner";
import { api } from "../api/client";

/*
 * Batch move, shared by drag&drop and the move dialog: folders travel as
 * folders (children keep their relative structure), files move one by one.
 * Moving a folder into itself or a descendant is refused up front, like
 * every OS file manager. The editor reopens the moved document only after
 * the listing refreshes.
 */
export function useMoveBatch({
  liveFiles,
  entryRevision,
  refreshFiles,
  fail,
  requestOpen,
  setBatchRunning,
  clearSelection,
  isEditorPath,
  t,
}: {
  liveFiles: { path: string }[];
  entryRevision: (path: string) => string | undefined;
  refreshFiles: () => Promise<void>;
  fail: (error: unknown) => void;
  requestOpen: (path: string) => void;
  setBatchRunning: (running: boolean) => void;
  /** Dropped paths go stale the moment the move lands. */
  clearSelection: () => void;
  /** Whether the open editor currently holds this path (it gets reopened). */
  isEditorPath: (path: string) => boolean;
  t: (key: string, vars?: Record<string, unknown>) => string;
}) {
  return (entries: { path: string; isFolder: boolean }[], into: string) => {
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
      clearSelection();
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
              if (isEditorPath(move.source)) movedOpenTarget = move.target;
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
            if (isEditorPath(entry.path)) movedOpenTarget = target;
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
}
