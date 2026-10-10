import { Eye, SquarePen, ArchiveRestore, Copy, FileWarning, LoaderCircle, Trash2, Trash2 as TrashIcon } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Skeleton } from "../ui/skeleton";
import { formatDateTime } from "../../lib/format";
import { api, isTextMedia, type Entry } from "../../api/client";
import { markdownToSafeHtml } from "../properties/preview";
import { newestContentful } from "./content";
import { Button } from "../ui/button";
import { Tooltip } from "../ui/tooltip";
import { ToggleGroupSection } from "../shell/toggle-section";

type ViewerContent =
  | { kind: "loading" }
  | { kind: "text"; html: string; raw: string }
  | { kind: "image"; url: string }
  | { kind: "binary" }
  | { kind: "unavailable" };

/*
 * Trash viewer — the middle pane for the selected tombstone: read-only
 * preview/source of its newest contentful revision, restore and copy-path
 * actions. Nothing here writes until 恢复 is pressed.
 */
export function TrashViewer({
  item,
  onRestore,
  onCopyPath,
  onPurge,
  purging,
  restoring,
}: {
  item: Entry | null;
  onRestore: (path: string) => void;
  onCopyPath: (path: string) => void;
  onPurge: (path: string) => void;
  purging: boolean;
  restoring: boolean;
}) {
  const { t } = useTranslation("workspace");
  const { t: tInfo } = useTranslation("info");
  const [viewMode, setViewMode] = useState<"preview" | "source">("preview");
  const [content, setContent] = useState<ViewerContent>({ kind: "loading" });
  useEffect(() => {
    if (!item) {
      setContent({ kind: "unavailable" });
      return;
    }
    let alive = true;
    setContent({ kind: "loading" });
    void (async () => {
      try {
        const found = await newestContentful(item.path);
        if (!alive) return;
        if (!found) {
          setContent({ kind: "unavailable" });
          return;
        }
        const meta = await api.readMeta(item.path).catch(() => null);
        const mediaType = meta?.media_type ?? "text/markdown";
        if (mediaType.startsWith("image/")) {
          const bytes = await api.readRevisionRaw(item.path, found.revision);
          const blob = new Blob([bytes as unknown as BlobPart], { type: mediaType });
          if (alive) setContent({ kind: "image", url: URL.createObjectURL(blob) });
          return;
        }
        const raw = await api.readRevisionText(item.path, found.revision);
        if (!alive) return;
        setContent({
          kind: "text",
          raw,
          html: isTextMedia(mediaType) ? await markdownToSafeHtml(raw) : "",
        });
      } catch {
        if (alive) setContent({ kind: "unavailable" });
      }
    })();
    return () => {
      alive = false;
    };
  }, [item]);

  if (!item) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 text-muted-foreground">
        <Trash2 className="size-5 stroke-[1.25] opacity-50" />
        <p className="text-body">{t("trashViewEmpty")}</p>
      </div>
    );
  }

  return (
    <div key={item.path} className="pane-in flex h-full min-h-0 flex-col bg-card" data-testid="trash-viewer">
      <div className="flex h-11 shrink-0 items-center gap-2 border-b px-2.5">
        <span className="shrink-0 rounded-full bg-warning/12 px-2 py-0.5 text-meta-sm text-warning">
          {t("panelTrash")}
        </span>
        <span className="min-w-0 flex-1 truncate font-mono text-xs text-muted-foreground" title={item.path}>
          {item.path}
        </span>
        <Tooltip content={t("restoreTooltip")}>
          <Button
            size="sm"
            variant="default"
            data-testid="trash-viewer-restore"
            aria-label={t("menuRestore")}
            disabled={restoring || purging}
            onClick={() => onRestore(item.path)}
          >
            {restoring ? <LoaderCircle className="animate-spin" /> : <ArchiveRestore />}
            <span className="hidden sm:inline">{t("menuRestore")}</span>
          </Button>
        </Tooltip>
        <Tooltip content={t("menuCopyPath")}>
          <Button size="icon" aria-label={t("menuCopyPath")} onClick={() => onCopyPath(item.path)}>
            <Copy />
          </Button>
        </Tooltip>
        <Tooltip content={t("purgeOne")}>
          <Button
            size="icon"
            variant="dangerGhost"
            aria-label={t("purgeOne")}
            data-testid="trash-viewer-purge"
            disabled={purging || restoring}
            onClick={() => onPurge(item.path)}
          >
            {purging ? <LoaderCircle className="animate-spin" /> : <TrashIcon />}
          </Button>
        </Tooltip>
      </div>
      {content.kind === "text" && (
        <div className="flex h-9 shrink-0 items-center border-b px-2.5">
          <ToggleGroupSection
            compactOnMobile
            value={viewMode}
            onChange={(next) => setViewMode(next as "preview" | "source")}
            options={[
              { value: "preview", label: tInfo("modePreview"), icon: <Eye className="size-3.5" /> },
              { value: "source", label: tInfo("modeCode"), icon: <SquarePen className="size-3.5" /> },
            ]}
          />
        </div>
      )}
      <div className="min-h-0 flex-1 overflow-auto">
        {content.kind === "loading" ? (
          <div className="flex h-full flex-col items-center justify-center gap-3 p-6">
            <Skeleton className="h-3 w-2/5" />
            <Skeleton className="h-3 w-1/3" />
            <Skeleton className="h-3 w-1/2" />
          </div>
        ) : content.kind === "image" ? (
          <div className="flex h-full items-center justify-center p-6">
            <img src={content.url} alt={item.path} className="max-h-full max-w-full object-contain" />
          </div>
        ) : content.kind === "text" ? (
          viewMode === "source" ? (
            <pre className="p-6 font-mono text-body-sm leading-relaxed whitespace-pre-wrap">
              {content.raw}
            </pre>
          ) : (
            <div
              className="markdown-preview"
              data-testid="trash-preview"
              dangerouslySetInnerHTML={{ __html: content.html }}
            />
          )
        ) : (
          <div className="flex h-full flex-col items-center justify-center gap-2 text-muted-foreground">
            <FileWarning className="size-5 stroke-[1.5] opacity-60" />
            <p className="text-xs">
              {content.kind === "binary" ? tInfo("previewUnavailable") : t("trashViewNoContent")}
            </p>
          </div>
        )}
      </div>
      <div className="flex h-8 shrink-0 items-center gap-2 border-t px-3 text-meta text-faint">
        <span>
          {t("trashDeletedAt", { time: formatDateTime(item.updated_at) })}
        </span>
      </div>
    </div>
  );
}
