import { LoaderCircle, TriangleAlert, Upload, X } from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  overwriteAll,
  setRowAction,
  type ExistingEntry,
  type UploadAction,
  type UploadPlan,
} from "./upload-conflict";
import { Button } from "../ui/button";
import { cn } from "../../lib/utils";

/*
 * Upload conflict diagnostic tree (inkmark's import-conflict-dialog adapted):
 * one row per decision-needed file, grouped by folder. Per-row 覆盖/重命名/
 * 跳过, an overwrite-all preset, and a running summary. Cancel aborts the
 * whole batch; confirm executes exactly what the rows say. Identical rows
 * are shown fixed as 无变化.
 */

export type UploadRowAction = UploadAction;

export function UploadConflictDialog({
  plan,
  existing,
  running,
  onCancel,
  onConfirm,
}: {
  plan: UploadPlan;
  /** The pre-upload entry list: rename targets must avoid these too. */
  existing: ExistingEntry[];
  running: boolean;
  onCancel: () => void;
  onConfirm: (plan: UploadPlan) => void;
}) {
  const { t } = useTranslation("workspace");
  const [rows, setRows] = useState(plan.rows);

  const conflictCount = rows.filter(
    (row) => row.existing && row.action !== "identical",
  ).length;
  const grouped = useMemo(() => {
    const groups = new Map<string, { row: (typeof rows)[number]; index: number }[]>();
    rows.forEach((row, index) => {
      if (!row.existing) return;
      const dir = row.incoming.path.includes("/")
        ? row.incoming.path.slice(0, row.incoming.path.lastIndexOf("/"))
        : "";
      const list = groups.get(dir) ?? [];
      list.push({ row, index });
      groups.set(dir, list);
    });
    return [...groups.entries()].sort(([a], [b]) => a.localeCompare(b));
  }, [rows]);

  const count = (action: UploadRowAction) => rows.filter((row) => row.action === action).length;
  const summaryParts = [
    count("overwrite") > 0 ? t("conflictSummaryOverwrite", { count: count("overwrite") }) : "",
    count("rename") > 0 ? t("conflictSummaryRename", { count: count("rename") }) : "",
    count("skip") > 0 ? t("conflictSummarySkip", { count: count("skip") }) : "",
    count("identical") > 0 ? t("conflictSummaryIdentical", { count: count("identical") }) : "",
    count("direct") > 0 ? t("conflictSummaryDirect", { count: count("direct") }) : "",
  ].filter(Boolean);
  const nothingToDo = rows.every((row) => row.action === "skip" || row.action === "identical");

  return (
    <>
      <div className="fixed inset-0 z-40 bg-black/35 backdrop-blur-[2px] fade-in" />
      <div
        className="fixed left-1/2 top-1/2 z-50 flex max-h-[86vh] w-[min(720px,94vw)] -translate-x-1/2 -translate-y-1/2 flex-col gap-3 rounded-[12px] border bg-card p-5 shadow-overlay overlay-in"
        role="dialog"
        aria-modal="true"
        aria-label={t("conflictTitle", { count: conflictCount })}
      >
      <div className="flex items-start gap-3">
        <div className="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-full bg-warning/10 text-warning">
          <TriangleAlert className="size-4 stroke-[1.5]" />
        </div>
        <div className="min-w-0 flex-1">
          <h2 className="text-[15px] font-semibold leading-snug tracking-tight">
            {t("conflictTitle", { count: conflictCount })}
          </h2>
          <p className="mt-1 text-[12.5px] leading-relaxed text-muted-foreground">
            {t("conflictHint")}
          </p>
        </div>
        <Button size="icon" aria-label={t("cancelUpload")} onClick={onCancel} disabled={running}>
          <X className="size-4 stroke-[1.5]" />
        </Button>
      </div>

      <div>
        <Button
          variant="outline"
          size="sm"
          onClick={() => setRows((prev) => overwriteAll(prev, existing))}
        >
          {t("conflictPresetOverwriteAll")}
        </Button>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto rounded-[8px] border">
        {grouped.length === 0 ? (
          <p className="px-3 py-6 text-center text-xs text-muted-foreground">
            {t("conflictEmpty")}
          </p>
        ) : (
          grouped.map(([dir, items]) => (
            <div key={dir}>
              <div className="border-b bg-sidebar px-3 pt-2 pb-1 text-[11px] font-medium text-faint">
                {dir === "" ? t("conflictRoot") : `${dir}/`}
              </div>
              {items.map(({ row, index }) => (
                <div
                  key={row.incoming.path}
                  className="flex items-center gap-2 border-b px-3 py-2 text-[13px] last:border-b-0"
                >
                  <span className="min-w-0 flex-1 truncate" title={row.incoming.path}>
                    {row.incoming.name}
                    {row.action === "rename" && row.renameTo && (
                      <span className="ml-1.5 font-mono text-[11px] text-muted-foreground">
                        → {row.renameTo}
                      </span>
                    )}
                  </span>
                  {row.action === "identical" ? (
                    <span className="shrink-0 rounded-full bg-muted px-2 py-0.5 text-[10.5px] text-muted-foreground">
                      {t("conflictIdenticalBadge")}
                    </span>
                  ) : (
                    <span className="flex shrink-0 items-center gap-0.5 rounded-[7px] border p-0.5">
                      {(
                        [
                          ["overwrite", t("conflictOverwrite")],
                          ["rename", t("conflictRename")],
                          ["skip", t("conflictSkip")],
                        ] as const
                      ).map(([action, label]) => (
                        <button
                          key={action}
                          type="button"
                          onClick={() =>
                            setRows((prev) => setRowAction(prev, existing, index, action))
                          }
                          aria-pressed={row.action === action}
                          className={cn(
                            "cursor-pointer rounded-md px-2 py-1 text-[11.5px] font-medium transition-colors",
                            row.action === action
                              ? "bg-foreground text-background"
                              : "text-muted-foreground hover:bg-muted hover:text-foreground",
                          )}
                        >
                          {label}
                        </button>
                      ))}
                    </span>
                  )}
                </div>
              ))}
            </div>
          ))
        )}
      </div>

      <div className="flex items-center justify-between gap-3">
        <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground">
          {summaryParts.length > 0 ? summaryParts.join(" · ") : t("conflictSummaryNone")}
        </span>
        <span className="flex shrink-0 items-center gap-2">
          {running && <LoaderCircle className="size-4 animate-spin stroke-[1.5] text-faint" />}
          <Button variant="outline" onClick={onCancel} disabled={running}>
            {t("cancelUpload")}
          </Button>
          <Button
            variant="default"
            data-testid="upload-confirm"
            disabled={nothingToDo || running}
            onClick={() => onConfirm({ ...plan, rows })}
          >
            <Upload className="size-4 stroke-[1.5]" />
            {t("confirmUpload")}
          </Button>
        </span>
      </div>
      </div>
    </>
  );
}
