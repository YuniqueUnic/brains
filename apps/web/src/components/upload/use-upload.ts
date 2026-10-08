import { useCallback, useRef, useState } from "react";
import { api, isTextMedia } from "../../api/client";
import {
  buildUploadPlan,
  type ExistingEntry,
  type IncomingFile,
  type UploadPlan,
  type UploadRow,
} from "./upload-conflict";

/*
 * Upload pipeline (DESIGN.md §35-37, inkmark UX): intake (picker, clipboard,
 * drag&drop) → conflict plan → optional diagnostic tree → sequential
 * execution with per-file progress. Pure orchestration; the workspace owns
 * toasts and the file-list refresh.
 */

export type UploadStatus = "waiting" | "uploading" | "done" | "failed" | "skipped";

export interface UploadItem {
  path: string;
  status: UploadStatus;
  note?: string;
}

export interface UploadSummary {
  done: number;
  failed: string[];
  skipped: number;
  identical: number;
}

async function sha256Hex(bytes: Uint8Array): Promise<string> {
  const buffer = bytes.buffer.slice(
    bytes.byteOffset,
    bytes.byteOffset + bytes.byteLength,
  ) as ArrayBuffer;
  const digest = await crypto.subtle.digest("SHA-256", buffer);
  return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, "0")).join("");
}

/** File → IncomingFile; the vault path comes from the intake kind. */
async function toIncoming(file: File, kind: "flat" | "tree"): Promise<IncomingFile | null> {
  const relative =
    kind === "tree" ? (file as File & { webkitRelativePath?: string }).webkitRelativePath : "";
  const name = file.name;
  // Folder uploads keep their relative path (strip the picked root folder
  // itself so "docs/…" lands at "docs/…", not "picked-root/docs/…").
  const path = relative && relative.includes("/") ? relative : name;
  const mediaType = file.type || "application/octet-stream";
  const sha256 = await sha256Hex(new Uint8Array(await file.arrayBuffer()));
  if (isTextMedia(mediaType) || /\.(md|txt|markdown|json|ya?ml|toml)$/i.test(name)) {
    try {
      const text = await file.text();
      return { path, name, text, bytes: null, mediaType: mediaType.startsWith("text/") ? mediaType : "text/markdown", sha256 };
    } catch {
      /* decode failed → treat as binary */
    }
  }
  return { path, name, text: null, bytes: await file.arrayBuffer(), mediaType, sha256 };
}

/** Recursively walk dropped directory entries into a flat File list. */
async function collectDropped(items: DataTransferItemList): Promise<File[]> {
  const files: File[] = [];
  const entries: FileSystemEntry[] = [];
  for (const item of Array.from(items)) {
    const entry = item.webkitGetAsEntry?.();
    if (entry) entries.push(entry);
  }
  const walk = async (entry: FileSystemEntry, prefix: string): Promise<void> => {
    if (entry.isFile) {
      const file = await new Promise<File>((resolve, reject) =>
        (entry as FileSystemFileEntry).file(resolve, reject),
      );
      Object.defineProperty(file, "webkitRelativePath", {
        value: `${prefix}${file.name}`,
        configurable: true,
      });
      files.push(file);
      return;
    }
    const reader = (entry as FileSystemDirectoryEntry).createReader();
    const children: FileSystemEntry[] = [];
    for (;;) {
      const batch = await new Promise<FileSystemEntry[]>((resolve, reject) =>
        reader.readEntries(resolve, reject),
      );
      if (batch.length === 0) break;
      children.push(...batch);
    }
    for (const child of children) {
      await walk(child, `${prefix}${entry.name}/`);
    }
  };
  for (const entry of entries) {
    await walk(entry, "");
  }
  return files;
}

export function useUpload({
  files,
  refresh,
  onFail,
}: {
  files: ExistingEntry[];
  refresh: () => Promise<void>;
  onFail: (error: unknown) => void;
}) {
  const [conflict, setConflict] = useState<{ plan: UploadPlan; existing: ExistingEntry[] } | null>(
    null,
  );
  const [progress, setProgress] = useState<UploadItem[] | null>(null);
  const [running, setRunning] = useState(false);
  const runIdRef = useRef(0);

  const execute = useCallback(
    async (rows: UploadRow[], runId: number): Promise<UploadSummary> => {
      const items: UploadItem[] = rows.map((row) => ({
        path: row.renameTo ?? row.incoming.path,
        status:
          row.action === "identical" || row.action === "skip"
            ? "skipped"
            : "waiting",
      }));
      const patch = (index: number, status: UploadStatus, note?: string) => {
        items[index] = { ...items[index], status, note };
        if (runIdRef.current === runId) setProgress([...items]);
      };
      if (runIdRef.current === runId) setProgress([...items]);
      const failed: string[] = [];
      let done = 0;
      for (let index = 0; index < rows.length; index += 1) {
        const row = rows[index];
        if (row.action === "identical" || row.action === "skip") continue;
        patch(index, "uploading");
        const target = row.renameTo ?? row.incoming.path;
        try {
          const file = row.incoming;
          const precondition =
            row.action === "overwrite" && row.existing
              ? ({ mode: "update", baseRevision: row.existing.revision } as const)
              : ({ mode: "create" } as const);
          if (file.text !== null) {
            await api.saveText(target, file.text, precondition, "text/markdown; charset=utf-8");
          } else if (file.bytes) {
            await api.saveBinary(target, file.bytes, precondition, file.mediaType);
          }
          done += 1;
          patch(index, "done");
        } catch (error) {
          failed.push(target);
          patch(index, "failed", error instanceof Error ? error.message : String(error));
        }
      }
      await refresh();
      return {
        done,
        failed,
        skipped: rows.filter((row) => row.action === "skip").length,
        identical: rows.filter((row) => row.action === "identical").length,
      };
    },
    [refresh],
  );

  const begin = useCallback(
    async (picked: File[] | DataTransferItemList, kind: "flat" | "tree") => {
      const runId = runIdRef.current + 1;
      runIdRef.current = runId;
      try {
        const list =
          picked instanceof DataTransferItemList ? await collectDropped(picked) : picked;
        if (list.length === 0) return;
        const incoming: IncomingFile[] = [];
        for (const file of list) {
          const converted = await toIncoming(file, kind);
          if (converted) incoming.push(converted);
        }
        if (incoming.length === 0) return;
        const plan = buildUploadPlan(incoming, files);
        if (plan.hasConflicts) {
          if (runIdRef.current === runId) setConflict({ plan, existing: files });
          return;
        }
        const summary = await execute(plan.rows, runId);
        if (runIdRef.current === runId) {
          setConflict(null);
          window.setTimeout(() => {
            if (runIdRef.current === runId) setProgress(null);
          }, 2500);
        }
        return summary;
      } catch (error) {
        onFail(error);
      }
    },
    [files, execute, onFail],
  );

  const confirm = useCallback(
    async (plan: UploadPlan): Promise<UploadSummary | undefined> => {
      if (!conflict) return undefined;
      const runId = runIdRef.current;
      setRunning(true);
      try {
        return await execute(plan.rows, runId);
      } finally {
        setRunning(false);
        setConflict(null);
        window.setTimeout(() => {
          if (runIdRef.current === runId) setProgress(null);
        }, 2500);
      }
    },
    [conflict, execute],
  );

  const cancel = useCallback(() => setConflict(null), []);
  const dismissProgress = useCallback(() => setProgress(null), []);

  return { conflict, progress, running, begin, confirm, cancel, dismissProgress };
}
