import { MarkdownContent, type MarkdownContext } from "../markdown/markdown-content";
import { ArchiveRestore, Link2, ListTree, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, type Entry, type RevisionDto } from "../../api/client";
import { previewFor, revisionPreviewFor, type FilePreview } from "./preview";
import { formatDateTime } from "../../lib/format";
import { Skeleton } from "../ui/skeleton";
import { HoverPreview } from "../ui/hover-preview";
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
import { RevisionHistory } from "./revision-history";

/*
 * Properties (DESIGN.md §43, #49): transient context opened with ⌘I / the
 * context menu / the editor toolbar button — never a permanent column.
 * Backlinks and history rows are live: hover for a rendered preview popover,
 * click to jump / view, restore only through confirmation.
 */

function BacklinkItem({ source, onOpenFile, context }: { source: string; onOpenFile: (path: string) => void; context: MarkdownContext }) {
  const { t } = useTranslation("info");
  return (
    <li>
      <HoverPreview
        context={{ ...context, path: source }}
        load={() => previewFor(source)}
        onOpen={() => onOpenFile(source)}
        trigger={
          <button
            type="button"
            onClick={() => onOpenFile(source)}
            className="block w-full cursor-pointer truncate rounded-md px-1.5 py-1 text-left font-mono text-meta text-muted-foreground transition-colors hover:bg-primary/10 hover:text-primary focus-visible:outline-2 focus-visible:outline-ring/70"
            title={t("openHint")}
          >
            {source}
          </button>
        }
      />
    </li>
  );
}

export function PropertiesPanel({
  path,
  entry,
  revision,
  onRestore,
  onClose,
  onOpenFile,
  files,
  onOpenTag,
}: {
  path: string | null;
  entry?: Entry;
  revision: string | null;
  onRestore: (revision: string) => void;
  onClose: () => void;
  onOpenFile: (path: string) => void;
  files: readonly { path: string }[];
  onOpenTag?: (tag: string) => void;
}) {
  const { t } = useTranslation("info");
  const context = { files, onOpen: onOpenFile, onTag: onOpenTag };
  const [fetchedMeta, setFetchedMeta] = useState<Entry | null>(null);
  const selectedRevision = revision ?? entry?.revision ?? null;
  const entryState = entry?.state;
  const hasProjectedMeta = Boolean(entry?.meta);
  const meta = entry?.meta ? entry : fetchedMeta?.path === path ? fetchedMeta : entry ?? null;
  // null = still loading (skeleton); loads that FAIL set the error flag so
  // the section says so instead of posing as an empty result.
  const [links, setLinks] = useState<{ outbound: number; backlinks: string[] } | null>(null);
  const [linksError, setLinksError] = useState(false);
  const [metaError, setMetaError] = useState(false);
  // Revision previews are read-only; restoring requires confirmation.
  const [viewing, setViewing] = useState<({ rev: RevisionDto } & FilePreview) | null>(null);
  const [viewMode, setViewMode] = useState<"preview" | "code">("preview");
  const [confirming, setConfirming] = useState<RevisionDto | null>(null);

  const pathRef = useRef(path);
  pathRef.current = path;
  const previewRunRef = useRef(0);
  const closePreview = () => {
    previewRunRef.current += 1;
    setViewing(null);
  };

  useEffect(() => {
    setFetchedMeta(null);
    setLinks(null);
    setViewing(null);
    setConfirming(null);
    previewRunRef.current += 1;
    if (!path) return;
    let alive = true;
    const controller = new AbortController();
    setMetaError(false);
    setLinksError(false);
    // Fetch lifecycle metadata only when the visible panel needs its projection.
    // Trashed paths use their listing tombstone; a raw metadata read returns 410.
    if (!hasProjectedMeta && entryState !== "trashed") {
      api.readMeta(path, controller.signal).then(
        result => alive && setFetchedMeta(result),
        () => {
          if (alive) setMetaError(true);
        },
      );
    }
    Promise.all([api.links(path, "outbound"), api.links(path, "backlinks")]).then(
      ([out, back]) => alive && setLinks({
        outbound: out.items.length,
        backlinks: back.items.map(item => item.source),
      }),
      () => alive && setLinksError(true),
    );
    return () => {
      alive = false;
      controller.abort();
      previewRunRef.current += 1;
    };
  }, [path, selectedRevision, entryState, hasProjectedMeta]);

  const openRevisionView = (rev: RevisionDto) => {
    if (!path) return;
    const requestedPath = path;
    const run = ++previewRunRef.current;
    setViewing({ rev, kind: "binary" });
    setViewMode("preview");
    const current = () => pathRef.current === requestedPath && previewRunRef.current === run;
    void revisionPreviewFor(path, rev.revision, rev.content?.media_type ?? "application/octet-stream").then(
      content => { if (current()) setViewing({ rev, ...content }); },
      () => { if (current()) setViewing({ rev, kind: "binary" }); },
    );
  };

  return (
    <aside
      className="flex h-full min-h-0 flex-col border-l bg-sidebar"
      data-testid="info-panel"
      aria-label={t("properties")}
    >
      <div className="flex h-11 shrink-0 items-center gap-2 border-b px-3">
        <h2 className="flex-1 text-body font-semibold">{t("properties")}</h2>
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
        <div key={path} aria-busy={meta === null && !metaError} className="pane-in min-h-0 flex-1 overflow-auto px-3 py-3">
          <section>
            <SectionTitle icon={<ListTree className="size-3.5 stroke-[1.5]" />} label={t("general")} />
            {meta === null && !metaError && (
              <div className="grid gap-1.5 py-1">
                <Skeleton className="h-3 w-3/4" />
                <Skeleton className="h-3 w-1/2" />
                <Skeleton className="h-3 w-2/3" />
              </div>
            )}
            {metaError && <p className="py-1 text-meta text-muted-foreground">{t("loadFailed")}</p>}
            <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs">
              <dt className="text-muted-foreground">{t("pathLabel")}</dt>
              <dd className="break-all font-mono text-meta">{path}</dd>
              {meta && (
                <>
                  <dt className="text-muted-foreground">{t("kindLabel")}</dt>
                  <dd className="font-mono text-meta">{kindLabel(t, meta.kind)}</dd>
                  {meta.size !== undefined && (
                    <>
                      <dt className="text-muted-foreground">{t("sizeLabel")}</dt>
                      <dd className="font-mono text-meta">{formatSize(meta.size)}</dd>
                    </>
                  )}
                  <dt className="text-muted-foreground">{t("generationLabel")}</dt>
                  <dd className="font-mono text-meta">{meta.generation}</dd>
                  {meta.updated_at && (
                    <>
                      <dt className="text-muted-foreground">{t("updatedLabel")}</dt>
                      <dd className="font-mono text-meta">{formatDateTime(meta.updated_at)}</dd>
                    </>
                  )}
                </>
              )}
              {meta?.meta && (
                <>
                  {meta.meta.type && (
                    <>
                      <dt className="text-muted-foreground">{t("noteTypeLabel")}</dt>
                      <dd className="font-mono text-meta">{meta.meta.type}</dd>
                    </>
                  )}
                  <dt className="text-muted-foreground">{t("statusLabel")}</dt>
                  <dd className="font-mono text-meta">{meta.meta.status}</dd>
                  <dt className="text-muted-foreground">{t("trustLabel")}</dt>
                  <dd className="font-mono text-meta">{meta.meta.trust}</dd>
                  {meta.meta.stale_after && (
                    <>
                      <dt className="text-muted-foreground">{t("staleAfterLabel")}</dt>
                      <dd className="font-mono text-meta">{meta.meta.stale_after}</dd>
                    </>
                  )}
                  {meta.meta.verified.length > 0 && (
                    <>
                      <dt className="text-muted-foreground">{t("verifiedLabel")}</dt>
                      <dd className="font-mono text-meta">
                        {meta.meta.verified
                          .map((v) => v.by)
                          .join(", ")}
                      </dd>
                    </>
                  )}
                  {meta.meta.sources.length > 0 && (
                    <>
                      <dt className="text-muted-foreground">{t("sourcesLabel")}</dt>
                      <dd className="font-mono text-meta">{meta.meta.sources.length}</dd>
                    </>
                  )}
                </>
              )}
            </dl>
          </section>

          <section className="mt-4">
            <SectionTitle icon={<Link2 className="size-3.5 stroke-[1.5]" />} label={t("links")} />
            {links === null && !linksError && (
              <div className="grid gap-1.5 py-1">
                <Skeleton className="h-3 w-2/5" />
              </div>
            )}
            {linksError && <p className="py-1 text-meta text-muted-foreground">{t("loadFailed")}</p>}
            {links !== null && (
              <p className="text-xs text-muted-foreground">
                {t("linkSummary", {
                  outbound: links.outbound,
                  backlinks: links.backlinks.length,
                })}
              </p>
            )}
            {links !== null && links.backlinks.length > 0 && (
              <ul className="mt-1 space-y-0.5">
                {links.backlinks.map((source) => (
                  <BacklinkItem context={context} key={source} source={source} onOpenFile={onOpenFile} />
                ))}
              </ul>
            )}
          </section>

          <RevisionHistory
            key={path}
            path={path}
            revision={selectedRevision}
            context={context}
            onView={openRevisionView}
            onRestore={setConfirming}
          />
        </div>
      )}

      {/* Read-only rendering of an old revision. */}
      <Dialog open={viewing !== null} onOpenChange={(open) => !open && closePreview()}>
        <DialogContent data-testid="revision-view-dialog" className="flex max-h-[80vh] w-[min(44rem,94vw)] flex-col">
          <DialogHeader>
            <DialogTitle>
              {t("viewRevisionTitle", {
                gen: viewing?.rev.generation ?? 0,
                time: formatDateTime(viewing?.rev.created_at ?? null),
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
                  className="max-h-[52vh] overflow-auto p-4 font-mono text-xs leading-relaxed whitespace-pre-wrap"
                >
                  {viewing.text}
                </pre>
              ) : (
                <MarkdownContent text={viewing.text} path={viewing.rev.path} {...context} className="!p-4" />
              )
            ) : (
              <p className="px-4 py-8 text-center text-xs text-muted-foreground">
                {t("previewUnavailable")}
              </p>
            )}
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={closePreview}>
              {t("close")}
            </Button>
            {viewing && viewing.rev.action !== "trash" && viewing.rev.revision !== revision && (
              <Button
                variant="default"
                data-testid="restore-from-view"
                onClick={() => {
                  setConfirming(viewing.rev);
                  closePreview();
                }}
              >
                <ArchiveRestore />
                {t("restoreRevision")}
              </Button>
            )}
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Restoring an old revision requires explicit confirmation. */}
      <AlertDialog open={confirming !== null} onOpenChange={(open) => !open && setConfirming(null)}>
        <AlertDialogContent data-testid="restore-revision-confirm">
          <AlertDialogTitle>{t("restoreConfirmTitle")}</AlertDialogTitle>
          <AlertDialogDescription>
            {t("restoreConfirmBody", {
              gen: confirming?.generation ?? 0,
              time: formatDateTime(confirming?.created_at ?? null),
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
    <h3 className="mb-1.5 flex items-center gap-1.5 text-meta font-semibold tracking-wide text-faint uppercase">
      <span>{icon}</span>
      {label}
    </h3>
  );
}

/** Server kinds ("note"/"text"/"script"/"binary") in UI words. */
function kindLabel(t: (key: string) => string, kind: string | undefined): string {
  if (kind === "note") return t("kindNote");
  if (kind === "text") return t("kindText");
  if (kind === "script") return t("kindScript");
  if (kind === "binary") return t("kindBinary");
  return kind ?? "-";
}

function formatSize(size: number): string {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
  return `${(size / 1024 / 1024).toFixed(1)} MB`;
}
