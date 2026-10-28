import { ArchiveRestore, ChevronRight, Eye, History } from "lucide-react";
import { useCallback, useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, type RevisionDto } from "../../api/client";
import { formatDateTime } from "../../lib/format";
import type { MarkdownContext } from "../markdown/markdown-content";
import { Button } from "../ui/button";
import { HoverPreview } from "../ui/hover-preview";
import { Skeleton } from "../ui/skeleton";
import { revisionPreviewFor } from "./preview";

type HistoryState = {
  items: RevisionDto[];
  cursor: string | null;
  status: "idle" | "loading" | "ready" | "error";
};

/** Revision pages are read only while expanded, one page per user action. */
export function RevisionHistory({ path, revision, context, onView, onRestore }: {
  path: string;
  revision: string | null;
  context: MarkdownContext;
  onView: (revision: RevisionDto) => void;
  onRestore: (revision: RevisionDto) => void;
}) {
  const { t } = useTranslation("info");
  const id = useId();
  const [expanded, setExpanded] = useState(false);
  const [page, setPage] = useState<HistoryState>({ items: [], cursor: null, status: "idle" });
  const controllerRef = useRef<AbortController | null>(null);
  const loadPage = useCallback(async (cursor: string | null, controller: AbortController) => {
    setPage(previous => ({
      items: cursor ? previous.items : [],
      cursor,
      status: "loading",
    }));
    try {
      const result = await api.history(path, cursor, controller.signal);
      if (controller.signal.aborted) return;
      setPage(previous => ({
        items: cursor ? [...previous.items, ...result.items] : result.items,
        cursor: result.next_cursor,
        status: "ready",
      }));
    } catch {
      if (!controller.signal.aborted) setPage(previous => ({ ...previous, status: "error" }));
    }
  }, [path]);

  useEffect(() => {
    if (!expanded) {
      setPage({ items: [], cursor: null, status: "idle" });
      return;
    }
    const controller = new AbortController();
    controllerRef.current = controller;
    void loadPage(null, controller);
    return () => controller.abort();
  }, [expanded, revision, loadPage]);

  const loading = page.status === "loading";
  return (
    <section className="mt-4">
      <button
        type="button"
        className="mb-2 flex w-full cursor-pointer items-center gap-1.5 rounded-md px-1.5 py-1 text-xs font-medium text-muted-foreground hover:bg-muted focus-visible:outline-2 focus-visible:outline-ring/70"
        aria-expanded={expanded}
        aria-controls={id}
        data-testid="history-toggle"
        onClick={() => setExpanded(value => !value)}
      >
        <History className="size-3.5 stroke-[1.5]" />
        {t("history")}
        <ChevronRight className={`ml-auto size-3.5 stroke-[1.5] transition-transform ${expanded ? "rotate-90" : ""}`} />
      </button>
      {expanded && (
        <div id={id} aria-busy={loading}>
          <ul className="space-y-0.5" data-testid="history-list">
            {loading && page.items.length === 0 && [0, 1, 2].map(index => (
              <li key={index} className="flex items-center gap-2 px-1.5 py-1">
                <Skeleton className="h-3 w-12" />
                <Skeleton className="h-3 w-10" />
                <Skeleton className="h-3 flex-1" />
              </li>
            ))}
            {page.items.map(item => (
              <li key={item.revision} className="flex items-center gap-2 rounded-md px-1.5 py-1 text-xs hover:bg-muted/70">
                {item.revision === revision || item.action === "trash" ? (
                  <>
                    <span className="shrink-0 font-mono text-meta text-muted-foreground">gen {item.generation}</span>
                    <span className="shrink-0">{actionLabel(t, item.action)}</span>
                    <span className="min-w-0 flex-1 truncate font-mono text-meta-sm text-faint">{formatDateTime(item.created_at)}</span>
                    {item.revision === revision ? (
                      <span className="shrink-0 rounded-full bg-primary/10 px-1.5 py-0.5 text-meta-xs text-primary">{t("current")}</span>
                    ) : <span className="shrink-0 text-meta-xs text-muted-foreground">{t("tombstone")}</span>}
                  </>
                ) : (
                  <HoverPreview
                    context={{ ...context, path }}
                    load={() => revisionPreviewFor(path, item.revision, item.content?.media_type)}
                    trigger={
                      <div className="flex min-w-0 flex-1 items-center gap-2">
                        <span className="shrink-0 font-mono text-meta text-muted-foreground">gen {item.generation}</span>
                        <span className="shrink-0">{actionLabel(t, item.action)}</span>
                        <span className="min-w-0 flex-1 truncate font-mono text-meta-sm text-faint">{formatDateTime(item.created_at)}</span>
                        <span className="flex shrink-0 items-center gap-0.5">
                          <button
                            data-testid={`view-revision-gen-${item.generation}`}
                            onClick={() => onView(item)}
                            aria-label={t("viewRevision")}
                            title={t("viewRevision")}
                            className="hit inline-flex cursor-pointer items-center gap-1 rounded-md px-1.5 py-0.5 text-meta-sm text-muted-foreground transition-colors hover:bg-primary/10 hover:text-primary focus-visible:outline-2 focus-visible:outline-ring/70"
                          >
                            <Eye className="size-3 stroke-[1.5]" />{t("viewRevision")}
                          </button>
                          <button
                            data-testid={`restore-revision-gen-${item.generation}`}
                            onClick={() => onRestore(item)}
                            className="hit inline-flex cursor-pointer items-center gap-1 rounded-md px-1.5 py-0.5 text-meta-sm text-muted-foreground transition-colors hover:bg-primary/10 hover:text-primary focus-visible:outline-2 focus-visible:outline-ring/70"
                          >
                            <ArchiveRestore className="size-3 stroke-[1.5]" />{t("restoreRevision")}
                          </button>
                        </span>
                      </div>
                    }
                  />
                )}
              </li>
            ))}
            {page.status === "error" && <li className="px-1.5 text-meta text-muted-foreground">{t("loadFailed")}</li>}
            {page.status === "ready" && page.items.length === 0 && <li className="px-1.5 text-meta text-muted-foreground">{t("noHistory")}</li>}
          </ul>
          {(page.cursor || page.status === "error") && (
            <Button
              variant="ghost" size="sm" className="mt-2" disabled={loading}
              data-testid="history-more"
              onClick={() => {
                const controller = controllerRef.current;
                if (controller && !controller.signal.aborted) void loadPage(page.cursor, controller);
              }}
            >
              {loading ? t("loadingHistory") : page.status === "error" ? t("retryHistory") : t("loadMoreHistory")}
            </Button>
          )}
        </div>
      )}
    </section>
  );
}

function actionLabel(t: (key: string) => string, action: string): string {
  if (action === "put") return t("actionPut");
  if (action === "restore") return t("actionRestore");
  if (action === "trash") return t("actionTrash");
  return action;
}
