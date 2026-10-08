import { ArchiveRestore, Copy, FileText, LoaderCircle, MoreHorizontal, Trash2 } from "lucide-react";
import { Menu } from "@base-ui-components/react/menu";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Entry } from "../../api/client";
import { cn } from "../../lib/utils";
import { Button } from "../ui/button";
import { menuItemClass, menuPopupClass } from "../ui/menu";

/*
 * Trash is its own workspace (DESIGN.md §38-42), not the file tree with a
 * flag: a page header with count, items grouped Today/Yesterday/Earlier,
 * and per-item restore. Brains has no permanent-delete by design (tombstones
 * are forever), so there is no "empty trash" — the honest UI omits it.
 * The e2e locators (file-list / .file-path / .restore-button) are preserved.
 */

type TrashGroup = { label: string; items: Entry[] };

function groupByDeletedTime(items: Entry[], t: (key: string) => string): TrashGroup[] {
  const startOfToday = new Date();
  startOfToday.setHours(0, 0, 0, 0);
  const yesterdayStart = startOfToday.getTime() - 24 * 3600 * 1000;
  const groups: Record<string, Entry[]> = { today: [], yesterday: [], earlier: [] };
  for (const item of items) {
    const time = Date.parse(item.updated_at ?? "");
    if (Number.isNaN(time) || time >= startOfToday.getTime()) groups.today.push(item);
    else if (time >= yesterdayStart) groups.yesterday.push(item);
    else groups.earlier.push(item);
  }
  return (
    [
      ["today", groups.today],
      ["yesterday", groups.yesterday],
      ["earlier", groups.earlier],
    ] as const
  )
    .filter(([, list]) => list.length > 0)
    .map(([key, list]) => ({ label: t(`trashGroup_${key}`), items: list }));
}

export function TrashView({
  items,
  onRestore,
  onBatchRestore,
  onCopyPath,
  restoringAll,
}: {
  items: Entry[];
  onRestore: (path: string) => void;
  onBatchRestore: () => void;
  onCopyPath: (path: string) => void;
  restoringAll: boolean;
}) {
  const { t } = useTranslation("workspace");
  const groups = useMemo(() => groupByDeletedTime(items, t), [items, t]);
  const [selected, setSelected] = useState<Set<string>>(new Set());

  const toggle = (path: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };

  return (
    <div className="min-h-0 flex-1 overflow-auto">
      <div className="mx-auto flex max-w-[860px] flex-col px-6 pt-10 pb-24">
        <div className="flex items-center gap-2">
          <h1 className="text-[21px] font-semibold tracking-tight">{t("panelTrash")}</h1>
          <span className="text-xs text-faint">{items.length}</span>
          <div className="ml-auto flex items-center gap-2">
            {restoringAll && (
              <LoaderCircle className="size-4 animate-spin stroke-[1.5] text-faint" />
            )}
            {selected.size > 0 ? (
              <>
                <span className="text-xs text-muted-foreground">
                  {t("selectedCount", { count: selected.size })}
                </span>
                <Button variant="outline" onClick={() => setSelected(new Set())}>
                  {t("clearSelection")}
                </Button>
                <Button
                  variant="secondary"
                  disabled={restoringAll}
                  data-testid="batch-restore"
                  onClick={() => {
                    onBatchRestore();
                    setSelected(new Set());
                  }}
                >
                  <ArchiveRestore className="size-4 stroke-[1.5]" />
                  {t("batchRestore")}
                </Button>
              </>
            ) : (
              <Button
                variant="outline"
                disabled={items.length === 0 || restoringAll}
                onClick={onBatchRestore}
              >
                <ArchiveRestore className="size-4 stroke-[1.5]" />
                {t("restoreAll")}
              </Button>
            )}
          </div>
        </div>
        <p className="mt-1 text-[12.5px] text-muted-foreground">{t("trashHint")}</p>

        {items.length === 0 ? (
          <div className="mt-16 flex flex-col items-center gap-2 text-muted-foreground">
            <Trash2 className="size-6 stroke-[1.25] opacity-50" />
            <p className="text-xs">{t("trashEmpty")}</p>
          </div>
        ) : (
          <div className="mt-6" data-testid="file-list">
            {groups.map((group) => (
              <section key={group.label} className="mt-5 first:mt-0">
                <h2 className="px-1 pb-1 text-[11px] font-semibold tracking-wide text-faint uppercase">
                  {group.label}
                </h2>
                <ul>
                  {group.items.map((item) => (
                    <li
                      key={item.path}
                      className={cn(
                        "group/row flex h-9 items-center gap-2.5 rounded-md px-2 text-[13px] transition-colors",
                        selected.has(item.path) ? "bg-selected" : "hover:bg-muted",
                      )}
                    >
                      <input
                        type="checkbox"
                        aria-label={t("selectOne", { path: item.path })}
                        className="size-3.5 shrink-0 accent-[var(--primary)]"
                        checked={selected.has(item.path)}
                        onChange={() => toggle(item.path)}
                      />
                      <FileText className="size-4 shrink-0 stroke-[1.5] text-faint" />
                      <span className="file-path min-w-0 flex-1 truncate">{item.path}</span>
                      <span className="shrink-0 text-[11px] text-faint">
                        {t("trashDeletedAt", { time: (item.updated_at ?? "").slice(0, 16) })}
                      </span>
                      <Menu.Root>
                        <Menu.Trigger
                          render={
                            <button
                              aria-label={t("rowMenu", { path: item.path })}
                              className="hidden size-6 shrink-0 cursor-pointer items-center justify-center rounded text-faint hover:bg-selected hover:text-foreground group-hover/row:flex focus-visible:outline-2 focus-visible:outline-ring/70"
                            />
                          }
                        >
                          <MoreHorizontal className="size-4 stroke-[1.5]" />
                        </Menu.Trigger>
                        <Menu.Portal>
                          <Menu.Positioner sideOffset={2} align="end">
                            <Menu.Popup className={menuPopupClass}>
                              <Menu.Item
                                className={menuItemClass()}
                                onClick={() => onRestore(item.path)}
                              >
                                <ArchiveRestore className="size-4 stroke-[1.5] text-muted-foreground" />
                                {t("menuRestore")}
                              </Menu.Item>
                              <Menu.Item
                                className={menuItemClass()}
                                onClick={() => onCopyPath(item.path)}
                              >
                                <Copy className="size-4 stroke-[1.5] text-muted-foreground" />
                                {t("menuCopyPath")}
                              </Menu.Item>
                            </Menu.Popup>
                          </Menu.Positioner>
                        </Menu.Portal>
                      </Menu.Root>
                      <button
                        className="restore-button flex h-7 shrink-0 cursor-pointer items-center gap-1 rounded-md px-2 text-[11.5px] text-muted-foreground opacity-0 transition-opacity hover:bg-primary/10 hover:text-primary focus-visible:opacity-100 focus-visible:outline-2 focus-visible:outline-ring/70 group-hover/row:opacity-100"
                        aria-label={t("restoreAria", { path: item.path })}
                        title={t("restoreTooltip")}
                        onClick={() => onRestore(item.path)}
                      >
                        <ArchiveRestore className="size-3.5 stroke-[1.5]" />
                        {t("menuRestore")}
                      </button>
                    </li>
                  ))}
                </ul>
              </section>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
