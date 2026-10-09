import { ArchiveRestore, FileText, LoaderCircle, Trash2, Trash2 as TrashIcon } from "lucide-react";
import { useTranslation } from "react-i18next";
import { formatDateTime } from "../../lib/format";
import type { Entry } from "../../api/client";
import { cn } from "../../lib/utils";
import { BulkSelectionBar, BulkSelectionToggle } from "../ui/bulk-selection-bar";
import { Button } from "../ui/button";

/*
 * Trash list (DESIGN.md §38-42) — the sidebar list of tombstoned items.
 * Rows keep the e2e locators: the container is [data-testid="file-list"],
 * names carry .file-path and the inline action .restore-button. Brains has
 * explicit batch controls for restore and confirmed permanent purge.
 */
export function TrashList({
  items,
  selectedPath,
  checked,
  selecting,
  onToggleSelecting,
  onSelectionChange,
  onSelect,
  onToggleSelect,
  onRestore,
  onRestoreAll,
  onPurgeChecked,
  restoringAll,
  purging,
}: {
  items: Entry[];
  selectedPath: string | null;
  checked: Set<string>;
  selecting: boolean;
  onToggleSelecting: () => void;
  onSelectionChange: (next: Set<string>) => void;
  onSelect: (path: string) => void;
  onToggleSelect: (path: string) => void;
  onRestore: (paths: string[]) => void;
  onRestoreAll: () => void;
  onPurgeChecked: (paths: string[]) => void;
  restoringAll: boolean;
  purging: boolean;
}) {
  const { t } = useTranslation("workspace");
  const sorted = [...items].sort((a, b) =>
    (b.updated_at ?? "").localeCompare(a.updated_at ?? ""),
  );

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex h-8 shrink-0 items-center gap-1 px-2">
        <span className="min-w-0 flex-1 truncate text-meta text-faint">{t("trashHintShort")}</span>
        <Button size="iconSm" aria-label={t("restoreAll")} title={t("restoreAll")}
          disabled={items.length === 0 || restoringAll || purging} onClick={onRestoreAll}>
          {restoringAll ? <LoaderCircle className="animate-spin" /> : <ArchiveRestore />}
        </Button>
        <BulkSelectionToggle selecting={selecting} onToggle={onToggleSelecting} disabled={purging || restoringAll} />
      </div>
      {selecting && <BulkSelectionBar paths={items.map((item) => item.path)} selected={checked}
        onSelectionChange={onSelectionChange} disabled={purging || restoringAll} actions={<>
          <Button size="iconSm" aria-label={t("restoreSelected")} title={t("restoreSelected")}
            disabled={checked.size === 0 || purging || restoringAll} onClick={() => onRestore([...checked])}><ArchiveRestore /></Button>
          <Button size="iconSm" variant="dangerGhost" aria-label={t("purgeSelected")} title={t("purgeSelected")}
            disabled={checked.size === 0 || purging || restoringAll} data-testid="purge-checked" onClick={() => onPurgeChecked([...checked])}>
            {purging ? <LoaderCircle className="animate-spin" /> : <TrashIcon />}
          </Button>
        </>} />}
      <div className="min-h-0 flex-1 overflow-auto px-1 pb-1" data-testid="file-list">
        {items.length === 0 ? (
          <div className="flex flex-col items-center gap-2 px-3 py-8 text-muted-foreground">
            <Trash2 className="size-5 stroke-[1.25] opacity-50" />
            <p className="text-xs">{t("trashEmpty")}</p>
          </div>
        ) : (
          <ul>
            {sorted.map((item) => (
              <li key={item.path}>
                <div
                  aria-current={selectedPath === item.path ? "true" : undefined}
                  aria-label={item.path}
                  tabIndex={0}
                  onKeyDown={(event) => {
                    if (event.target !== event.currentTarget || purging || restoringAll) return;
                    if (event.key === "Enter" || event.key === " ") {
                      event.preventDefault();
                      if (selecting) onToggleSelect(item.path); else onSelect(item.path);
                    }
                  }}
                  onClick={() => { if (!purging && !restoringAll) { if (selecting) onToggleSelect(item.path); else onSelect(item.path); } }}
                  title={t("trashDeletedAt", { time: formatDateTime(item.updated_at) })}
                  className={cn(
                    "group/row flex h-7 cursor-pointer select-none items-center gap-1.5 rounded-md pr-1 text-body transition-colors focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring/70",
                    selectedPath === item.path
                      ? "bg-selected text-foreground"
                      : "hover:bg-muted",
                  )}
                >
                  {selecting && <input
                    type="checkbox"
                    aria-label={t("selectOne", { path: item.path })}
                    className="size-3.5 shrink-0 accent-[var(--primary)]"
                    disabled={purging || restoringAll}
                    checked={checked.has(item.path)}
                    onClick={(event) => event.stopPropagation()}
                    onChange={() => onToggleSelect(item.path)}
                  />}
                  <FileText className="size-4 shrink-0 stroke-[1.5] text-faint" />
                  <span className="file-path min-w-0 flex-1 truncate">{item.path}</span>
                  <button
                    className="restore-button ml-auto flex h-6 shrink-0 cursor-pointer items-center gap-1 rounded-md px-1.5 text-meta-sm text-muted-foreground opacity-100 sm:opacity-0 hover:bg-primary/10 hover:text-primary focus-visible:opacity-100 focus-visible:outline-2 focus-visible:outline-ring/70 group-hover/row:opacity-100"
                    aria-label={t("restoreAria", { path: item.path })}
                    disabled={purging || restoringAll}
                    onClick={(event) => {
                      event.stopPropagation();
                      onRestore([item.path]);
                    }}
                  >
                    <ArchiveRestore className="size-3 stroke-[1.5]" />
                    <span className="hidden sm:inline">{t("menuRestore")}</span>
                  </button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
