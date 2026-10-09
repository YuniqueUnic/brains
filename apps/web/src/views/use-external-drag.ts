import { useEffect, useState } from "react";
import type { useUpload } from "../components/upload/use-upload";
import type { WorkspaceMode } from "../components/shell/sidebar";
import type { UploadSummary } from "../components/upload/use-upload";

/*
 * External file drag intake (the import overlay on the workspace root): only
 * OS-level file drags participate — the tree's own move-drag carries a custom
 * payload and must not trigger it. Drop sources that expose plain files but
 * no entry objects fall back to the flat file list.
 */
export function useExternalDrag({
  mode,
  upload,
  runUpload,
}: {
  mode: WorkspaceMode;
  upload: ReturnType<typeof useUpload>;
  runUpload: (summary: UploadSummary | undefined) => void;
}) {
  const [dragging, setDragging] = useState(false);
  useEffect(() => {
    const clearDrag = () => setDragging(false);
    window.addEventListener("dragend", clearDrag);
    window.addEventListener("drop", clearDrag);
    return () => {
      window.removeEventListener("dragend", clearDrag);
      window.removeEventListener("drop", clearDrag);
    };
  }, []);
  const overlayHandlers = {
    onDragOver: (event: React.DragEvent) => {
      if (mode !== "workspace") return;
      if (!event.dataTransfer.types.includes("Files")) return;
      event.preventDefault();
      setDragging(true);
    },
    onDragLeave: (event: React.DragEvent) => {
      if (
        !event.relatedTarget ||
        !(event.currentTarget as HTMLElement).contains(event.relatedTarget as Node)
      ) {
        setDragging(false);
      }
    },
    onDrop: (event: React.DragEvent) => {
      setDragging(false);
      if (mode !== "workspace") return;
      if (!event.dataTransfer.types.includes("Files")) return;
      event.preventDefault();
      const transfer = event.dataTransfer;
      void upload
        .begin(transfer.items, "tree")
        .then((summary) => {
          if (summary === undefined && transfer.files.length > 0) {
            return upload.begin(Array.from(transfer.files), "tree");
          }
          return summary;
        })
        .then(runUpload);
    },
  };
  return { dragging, overlayHandlers };
}
