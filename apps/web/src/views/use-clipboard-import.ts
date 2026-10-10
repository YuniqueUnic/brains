import { useEffect, useRef } from "react";
import { toast } from "sonner";
import type { UploadSummary, useUpload } from "../components/upload/use-upload";
import type { WorkspaceMode } from "../components/shell/sidebar";

/*
 * Clipboard intake: paste anywhere outside a text field, or the 上传 menu
 * item. Images land as unnamed.png (+N), text as unnamed.md — synthetic
 * names, so the rename plan lands silently (autoConfirm). Real files picked
 * or dropped keep the conflict dialog.
 */
export function useClipboardImport({
  upload,
  mode,
  t,
}: {
  upload: ReturnType<typeof useUpload>;
  mode: WorkspaceMode;
  t: (key: string, vars?: Record<string, unknown>) => string;
}) {
  const runUpload = (summary: UploadSummary | undefined) => {
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
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return { runUpload, importFromClipboard };
}
