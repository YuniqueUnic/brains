import { ArchiveRestore, FileText, LoaderCircle, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { Entry } from "../../api/client";
import { cn } from "../../lib/utils";
import { Button } from "../ui/button";

/*
 * Trash list (DESIGN.md §38-42) — the sidebar list of tombstoned items.
 * Rows keep the e2e locators: the container is [data-testid="file-list"],
 * names carry .file-path and the inline action .restore-button. Brains has
 * no permanent delete (tombstones are forever), so there is no empty-trash.
 */
export function TrashList({
  items,
  selectedPath,
  onSelect,
  onRestore,
  onRestoreAll,
  restoringAll,
}: {
  items: Entry[];
  selectedPath: string | null;
  onSelect: (path: string) => void;
  onRestore: (path: string) => void;
  onRestoreAll: () => void;
  restoringAll: boolean;
}) {
  const { t } = useTranslation("workspace");
  const sorted = [...items].sort((a, b) =>
    (b.updated_at ?? "").localeCompare(a.updated_at ?? ""),
  );

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex h-8 shrink-0 items-center gap-1 px-2">
        <span className="min-w-0 flex-1 truncate text-[11px] text-faint">
          {t("trashHintShort")}
        </span>
        <Button
          variant="ghost"
          size="iconSm"
          aria-label={t("restoreAll")}
          title={t("restoreAll")}
          disabled={items.length === 0 || restoringAll}
          onClick={onRestoreAll}
        >
          {restoringAll ? (
            <LoaderCircle className="animate-spin" />
          ) : (
            <ArchiveRestore />
          )}
        </Button>
      </div>
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
                  role="row"
                  aria-label={item.path}
                  onClick={() => onSelect(item.path)}
                  title={t("trashDeletedAt", { time: (item.updated_at ?? "").slice(0, 19) })}
                  className={cn(
                    "group/row flex h-7 cursor-pointer select-none items-center gap-1.5 rounded-md pr-1 text-[13px] transition-colors",
                    selectedPath === item.path
                      ? "bg-selected text-foreground"
                      : "hover:bg-muted",
                  )}
                >
                  <FileText className="size-4 shrink-0 stroke-[1.5] text-faint" />
                  <span className="file-path min-w-0 flex-1 truncate">{item.path}</span>
                  <button
                    className="restore-button ml-auto flex h-6 shrink-0 cursor-pointer items-center gap-1 rounded-md px-1.5 text-[10.5px] text-muted-foreground opacity-0 hover:bg-primary/10 hover:text-primary focus-visible:opacity-100 focus-visible:outline-2 focus-visible:outline-ring/70 group-hover/row:opacity-100"
                    aria-label={t("restoreAria", { path: item.path })}
                    onClick={(event) => {
                      event.stopPropagation();
                      onRestore(item.path);
                    }}
                  >
                    <ArchiveRestore className="size-3 stroke-[1.5]" />
                    {t("menuRestore")}
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
