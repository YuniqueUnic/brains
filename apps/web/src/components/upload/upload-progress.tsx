import { Check, LoaderCircle, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { UploadItem } from "./use-upload";
import { cn } from "../../lib/utils";

/** Bottom-right upload progress card (DESIGN.md §37): never a page change. */
export function UploadProgress({
  items,
  onDismiss,
}: {
  items: UploadItem[];
  onDismiss: () => void;
}) {
  const { t } = useTranslation("workspace");
  const done = items.filter((item) => item.status === "done").length;
  const total = items.length;
  const finished = items.every(
    (item) => item.status === "done" || item.status === "failed" || item.status === "skipped",
  );
  return (
    <div className="fixed bottom-3 right-3 z-40 w-[300px] overflow-hidden rounded-[10px] border bg-card shadow-overlay fade-in">
      <div className="flex items-center gap-2 border-b px-3 py-2">
        <span className="flex-1 text-[12.5px] font-semibold">
          {finished ? t("uploadDone", { done, total }) : t("uploadRunning", { done, total })}
        </span>
        {finished && (
          <button
            aria-label={t("close")}
            onClick={onDismiss}
            className="cursor-pointer rounded p-0.5 text-muted-foreground hover:bg-muted hover:text-foreground"
          >
            <X className="size-3.5 stroke-[1.5]" />
          </button>
        )}
      </div>
      <div className="h-0.5 bg-muted">
        <div
          className="h-full bg-primary transition-[width] duration-300"
          style={{ width: `${total === 0 ? 100 : (done / total) * 100}%` }}
        />
      </div>
      <ul className="max-h-44 overflow-y-auto p-1">
        {items.map((item) => (
          <li
            key={item.path}
            className="flex items-center gap-2 rounded-md px-2 py-1 text-[11.5px]"
            title={item.note}
          >
            <span className="min-w-0 flex-1 truncate font-mono">{item.path}</span>
            {item.status === "uploading" && (
              <LoaderCircle className="size-3.5 shrink-0 animate-spin stroke-[1.5] text-faint" />
            )}
            {item.status === "done" && (
              <Check className="size-3.5 shrink-0 stroke-[1.5] text-success" />
            )}
            {item.status === "skipped" && (
              <span className="shrink-0 text-faint">{t("uploadSkipped")}</span>
            )}
            {item.status === "waiting" && (
              <span className="shrink-0 text-faint">{t("uploadWaiting")}</span>
            )}
            {item.status === "failed" && (
              <span className={cn("shrink-0 text-destructive")}>{t("uploadFailed")}</span>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}
