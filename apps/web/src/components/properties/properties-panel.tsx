import { ArchiveRestore, Eye, History, Link2, ListTree, X } from "lucide-react";
import { Tooltip as TooltipPrimitive } from "@base-ui-components/react/tooltip";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, type Entry, type RevisionDto } from "../../api/client";
import {
  markdownToSafeHtml,
  previewFor,
  revisionPreviewFor,
  type PreviewResult,
} from "./preview";
import { ToggleGroupSection } from "../shell/toggle-section";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogTitle,
} from "../ui/alert-dialog";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "../ui/dialog";
import { Button } from "../ui/button";

/*
 * Properties (DESIGN.md §43, #49): transient context opened with ⌘I / the
 * context menu / the editor toolbar button — never a permanent column.
 * Backlinks and history rows are live: hover for a rendered preview popover,
 * click to jump / view, restore only through confirmation.
 */

/** Hover popover body shared by backlink and history triggers. */
function PreviewBody({ path, preview }: { path: string; preview: PreviewResult | null }) {
  const { t } = useTranslation("info");
  return (
    <div className="flex h-auto max-h-[320px] items-start justify-center overflow-auto">
      {preview === null ? (
        <p className="px-4 py-6 text-xs text-muted-foreground">{t("previewLoading")}</p>
      ) : preview.kind === "image" ? (
        <img src={preview.url} alt={path} className="max-h-[320px] w-auto object-contain" />
      ) : preview.kind === "text" ? (
        <div
          className="markdown-preview w-full !py-4 text-[13px]"
          dangerouslySetInnerHTML={{ __html: preview.html }}
        />
      ) : (
        <p className="px-4 py-6 text-xs text-muted-foreground">{t("previewUnavailable")}</p>
      )}
    </div>
  );
}

function BacklinkItem({ source, onOpenFile }: { source: string; onOpenFile: (path: string) => void }) {
  const { t } = useTranslation("info");
  const [preview, setPreview] = useState<PreviewResult | null>(null);
  return (
    <li>
      <TooltipPrimitive.Root>
        <TooltipPrimitive.Trigger
          render={
            <button
              type="button"
              onClick={() => onOpenFile(source)}
              className="block w-full cursor-pointer truncate rounded-md px-1.5 py-1 text-left font-mono text-[11px] text-muted-foreground transition-colors hover:bg-primary/10 hover:text-primary focus-visible:outline-2 focus-visible:outline-ring/70"
              title={t("openHint")}
            >
              {source}
            </button>
          }
          onMouseEnter={() => {
            void previewFor(source).then(setPreview, () => setPreview({ kind: "binary" }));
          }}
        />
        <TooltipPrimitive.Portal>
          <TooltipPrimitive.Positioner side="left" sideOffset={10} align="start">
            <TooltipPrimitive.Popup className="z-50 max-h-[320px] w-[min(360px,80vw)] overflow-hidden rounded-[10px] border bg-card shadow-overlay">
              <PreviewBody path={source} preview={preview} />
            </TooltipPrimitive.Popup>
          </TooltipPrimitive.Positioner>
        </TooltipPrimitive.Portal>
      </TooltipPrimitive.Root>
    </li>
  );
}

/** Hoverable wrapper giving a history row the same rendered preview popover. */
function HistoryRowTooltip({
  path,
  revision,
  children,
}: {
  path: string;
  revision: string;
  children: React.ReactNode;
}) {
  const [preview, setPreview] = useState<PreviewResult | null>(null);
  return (
    <TooltipPrimitive.Root>
      <TooltipPrimitive.Trigger
        render={<div className="flex min-w-0 flex-1 items-center gap-2">{children}</div>}
        onMouseEnter={() => {
          void revisionPreviewFor(path, revision).then(
            setPreview,
            () => setPreview({ kind: "binary" }),
          );
        }}
      />
      <TooltipPrimitive.Portal>
        <TooltipPrimitive.Positioner side="left" sideOffset={10} align="start">
          <TooltipPrimitive.Popup className="z-50 max-h-[320px] w-[min(360px,80vw)] overflow-hidden rounded-[10px] border bg-card shadow-overlay" role="tooltip">
            <PreviewBody path={`${path}@${revision.slice(0, 8)}`} preview={preview} />
          </TooltipPrimitive.Popup>
        </TooltipPrimitive.Positioner>
      </TooltipPrimitive.Portal>
    </TooltipPrimitive.Root>
  );
}

export function PropertiesPanel({
  path,
  revision,
  onRestore,
  onClose,
  onOpenFile,
}: {
  path: string | null;
  revision: string | null;
  onRestore: (revision: string) => void;
  onClose: () => void;
  onOpenFile: (path: string) => void;
}) {
  const { t } = useTranslation("info");
  const [meta, setMeta] = useState<Entry | null>(null);
  const [history, setHistory] = useState<RevisionDto[]>([]);
  const [links, setLinks] = useState<{ outbound: number; backlinks: string[] }>({
    outbound: 0,
    backlinks: [],
  });
  // 查看: rendered/raw content of an old revision. 恢复: destructive, confirmed.
  const [viewing, setViewing] = useState<
    | { rev: RevisionDto; kind: "text"; html: string; raw: string }
    | { rev: RevisionDto; kind: "image"; url: string }
    | { rev: RevisionDto; kind: "binary" }
    | null
  >(null);
  const [viewMode, setViewMode] = useState<"preview" | "code">("preview");
  const [confirming, setConfirming] = useState<RevisionDto | null>(null);

  useEffect(() => {
    if (!path) {
      setMeta(null);
      setHistory([]);
      return;
    }
    let alive = true;
    api.readMeta(path).then(
      (entry) => alive && setMeta(entry),
      () => alive && setMeta(null),
    );
    collectHistory(path).then((items) => alive && setHistory(items));
    Promise.all([api.links(path, "outbound"), api.links(path, "backlinks")]).then(
      ([out, back]) =>
        alive &&
        setLinks({
          outbound: out.items.length,
          backlinks: back.items.map((item) => item.source),
        }),
      () => undefined,
    );
    return () => {
      alive = false;
    };
  }, [path, revision]);

  const openRevisionView = (rev: RevisionDto) => {
    if (!path) return;
    setViewing({ rev, kind: "binary" });
    setViewMode("preview");
    void (async () => {
      try {
        if (rev.content?.media_type?.startsWith("image/")) {
          const bytes = await api.readRevisionRaw(path, rev.revision);
          const blob = new Blob([bytes as unknown as BlobPart], {
            type: rev.content.media_type,
          });
          setViewing({ rev, kind: "image", url: URL.createObjectURL(blob) });
          return;
        }
        const text = await api.readRevisionText(path, rev.revision);
        setViewing({ rev, kind: "text", html: await markdownToSafeHtml(text), raw: text });
      } catch {
        setViewing({ rev, kind: "binary" });
      }
    })();
  };

  return (
    <aside
      className="flex h-full min-h-0 flex-col border-l bg-sidebar"
      data-testid="info-panel"
      aria-label={t("properties")}
    >
      <div className="flex h-11 shrink-0 items-center gap-2 border-b px-3">
        <h2 className="flex-1 text-[13px] font-semibold">{t("properties")}</h2>
        <kbd aria-hidden>⌘I</kbd>
        <button
          aria-label={t("close")}
          onClick={onClose}
          className="cursor-pointer rounded-md p-1.5 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring/70"
        >
          <X className="size-4 stroke-[1.5]" />
        </button>
      </div>

      {!path ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-2 text-muted-foreground">
          <ListTree className="size-5 stroke-[1.25] opacity-60" />
          <p className="text-xs">{t("noSelection")}</p>
        </div>
      ) : (
        <div className="min-h-0 flex-1 overflow-auto px-3 py-3">
          <section>
            <SectionTitle icon={<ListTree className="size-3.5 stroke-[1.5]" />} label={t("general")} />
            <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-[12px]">
              <dt className="text-muted-foreground">{t("pathLabel")}</dt>
              <dd className="break-all font-mono text-[11px]">{path}</dd>
              {meta && (
                <>
                  <dt className="text-muted-foreground">{t("kindLabel")}</dt>
                  <dd className="font-mono text-[11px]">{meta.kind ?? "-"}</dd>
                  {meta.size !== undefined && (
                    <>
                      <dt className="text-muted-foreground">{t("sizeLabel")}</dt>
                      <dd className="font-mono text-[11px]">{formatSize(meta.size)}</dd>
                    </>
                  )}
                  <dt className="text-muted-foreground">generation</dt>
                  <dd className="font-mono text-[11px]">{meta.generation}</dd>
                  {meta.updated_at && (
                    <>
                      <dt className="text-muted-foreground">{t("updatedLabel")}</dt>
                      <dd className="font-mono text-[11px]">{meta.updated_at.slice(0, 19)}</dd>
                    </>
                  )}
                </>
              )}
            </dl>
          </section>

          <section className="mt-4">
            <SectionTitle icon={<Link2 className="size-3.5 stroke-[1.5]" />} label={t("links")} />
            <p className="text-[12px] text-muted-foreground">
              {t("linkSummary", {
                outbound: links.outbound,
                backlinks: links.backlinks.length,
              })}
            </p>
            {links.backlinks.length > 0 && (
              <ul className="mt-1 space-y-0.5">
                {links.backlinks.map((source) => (
                  <BacklinkItem key={source} source={source} onOpenFile={onOpenFile} />
                ))}
              </ul>
            )}
          </section>

          <section className="mt-4">
            <SectionTitle icon={<History className="size-3.5 stroke-[1.5]" />} label={t("history")} />
            <ul className="space-y-0.5" data-testid="history-list">
              {history.map((rev) => (
                <li
                  key={rev.revision}
                  className="flex items-center gap-2 rounded-md px-1.5 py-1 text-[12px] hover:bg-muted/70"
                >
                  {rev.revision === revision ? (
                    <>
                      <span className="shrink-0 font-mono text-[11px] text-muted-foreground">
                        gen {rev.generation}
                      </span>
                      <span className="shrink-0">{rev.action}</span>
                      <span className="min-w-0 flex-1 truncate font-mono text-[10.5px] text-faint">
                        {rev.created_at.slice(0, 19)}
                      </span>
                      <span className="shrink-0 rounded-full bg-primary/10 px-1.5 py-0.5 text-[10px] text-primary">
                        {t("current")}
                      </span>
                    </>
                  ) : rev.action === "trash" ? (
                    // A tombstone has no content to restore; the trash view's
                    // restore walks history for the newest contentful revision.
                    <>
                      <span className="shrink-0 font-mono text-[11px] text-muted-foreground">
                        gen {rev.generation}
                      </span>
                      <span className="shrink-0">{rev.action}</span>
                      <span className="min-w-0 flex-1 truncate font-mono text-[10.5px] text-faint">
                        {rev.created_at.slice(0, 19)}
                      </span>
                      <span className="shrink-0 text-[10px] text-muted-foreground">
                        {t("tombstone")}
                      </span>
                    </>
                  ) : (
                    <HistoryRowTooltip path={path} revision={rev.revision}>
                      <span className="shrink-0 font-mono text-[11px] text-muted-foreground">
                        gen {rev.generation}
                      </span>
                      <span className="shrink-0">{rev.action}</span>
                      <span className="min-w-0 flex-1 truncate font-mono text-[10.5px] text-faint">
                        {rev.created_at.slice(0, 19)}
                      </span>
                      <span className="flex shrink-0 items-center gap-0.5">
                        <button
                          data-testid={`view-revision-gen-${rev.generation}`}
                          onClick={() => openRevisionView(rev)}
                          aria-label={t("viewRevision")}
                          title={t("viewRevision")}
                          className="inline-flex cursor-pointer items-center gap-1 rounded-md px-1.5 py-0.5 text-[10.5px] text-muted-foreground transition-colors hover:bg-primary/10 hover:text-primary focus-visible:outline-2 focus-visible:outline-ring/70"
                        >
                          <Eye className="size-3 stroke-[1.5]" />
                          {t("viewRevision")}
                        </button>
                        <button
                          data-testid={`restore-revision-gen-${rev.generation}`}
                          onClick={() => setConfirming(rev)}
                          className="inline-flex cursor-pointer items-center gap-1 rounded-md px-1.5 py-0.5 text-[10.5px] text-muted-foreground transition-colors hover:bg-primary/10 hover:text-primary focus-visible:outline-2 focus-visible:outline-ring/70"
                        >
                          <ArchiveRestore className="size-3 stroke-[1.5]" />
                          {t("restoreRevision")}
                        </button>
                      </span>
                    </HistoryRowTooltip>
                  )}
                </li>
              ))}
              {history.length === 0 && (
                <li className="px-1.5 text-[11px] text-muted-foreground">{t("noHistory")}</li>
              )}
            </ul>
          </section>
        </div>
      )}

      {/* 查看: read-only rendering of an old revision. */}
      <Dialog open={viewing !== null} onOpenChange={(open) => !open && setViewing(null)}>
        <DialogContent data-testid="revision-view-dialog" className="flex max-h-[80vh] w-[min(44rem,94vw)] flex-col">
          <DialogHeader>
            <DialogTitle>
              {t("viewRevisionTitle", {
                gen: viewing?.rev.generation ?? 0,
                time: viewing?.rev.created_at.slice(0, 19) ?? "",
              })}
            </DialogTitle>
            <DialogDescription>
              {viewing?.rev.content?.media_type ?? ""}
            </DialogDescription>
          </DialogHeader>
          {viewing?.kind === "text" && (
            <div className="flex items-center">
              <ToggleGroupSection
                value={viewMode}
                onChange={(next) => setViewMode(next as "preview" | "code")}
                options={[
                  { value: "preview", label: t("modePreview") },
                  { value: "code", label: t("modeCode") },
                ]}
              />
            </div>
          )}
          <div className="min-h-0 flex-1 overflow-auto rounded-md border bg-background">
            {viewing?.kind === "image" ? (
              <div className="flex items-center justify-center p-4">
                <img src={viewing.url} alt={viewing.rev.path} className="max-h-[52vh] w-auto object-contain" />
              </div>
            ) : viewing?.kind === "text" ? (
              viewMode === "code" ? (
                <pre
                  data-testid="revision-view-code"
                  className="max-h-[52vh] overflow-auto p-4 font-mono text-[12px] leading-relaxed whitespace-pre-wrap"
                >
                  {viewing.raw}
                </pre>
              ) : (
                <div
                  className="markdown-preview !p-4"
                  dangerouslySetInnerHTML={{ __html: viewing.html }}
                />
              )
            ) : (
              <p className="px-4 py-8 text-center text-xs text-muted-foreground">
                {t("previewUnavailable")}
              </p>
            )}
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setViewing(null)}>
              {t("close")}
            </Button>
            {viewing && viewing.rev.action !== "trash" && viewing.rev.revision !== revision && (
              <Button
                variant="default"
                data-testid="restore-from-view"
                onClick={() => {
                  setConfirming(viewing.rev);
                  setViewing(null);
                }}
              >
                <ArchiveRestore />
                {t("restoreRevision")}
              </Button>
            )}
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 恢复: destructive → confirmed (design §59 danger levels). */}
      <AlertDialog open={confirming !== null} onOpenChange={(open) => !open && setConfirming(null)}>
        <AlertDialogContent data-testid="restore-revision-confirm">
          <AlertDialogTitle>{t("restoreConfirmTitle")}</AlertDialogTitle>
          <AlertDialogDescription>
            {t("restoreConfirmBody", {
              gen: confirming?.generation ?? 0,
              time: confirming?.created_at.slice(0, 19) ?? "",
            })}
          </AlertDialogDescription>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("cancel")}</AlertDialogCancel>
            <AlertDialogAction
              data-testid="restore-revision-confirm-yes"
              onClick={() => {
                const target = confirming?.revision ?? null;
                setConfirming(null);
                if (target) onRestore(target);
              }}
            >
              {t("restoreConfirmAction")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </aside>
  );
}

function SectionTitle({ icon, label }: { icon: React.ReactNode; label: string }) {
  return (
    <h3 className="mb-1.5 flex items-center gap-1.5 text-[11px] font-semibold tracking-wide text-faint uppercase">
      <span>{icon}</span>
      {label}
    </h3>
  );
}

function formatSize(size: number): string {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
  return `${(size / 1024 / 1024).toFixed(1)} MB`;
}

async function collectHistory(path: string, maxPages = 10): Promise<RevisionDto[]> {
  const items: RevisionDto[] = [];
  let cursor: string | null | undefined = undefined;
  for (let page = 0; page < maxPages; page += 1) {
    const result = await api.history(path, cursor ?? undefined);
    items.push(...result.items);
    if (!result.next_cursor) break;
    cursor = result.next_cursor;
  }
  return items;
}
