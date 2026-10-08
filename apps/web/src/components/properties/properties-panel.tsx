import { ArchiveRestore, History, Link2, ListTree, X } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, type Entry, type RevisionDto } from "../../api/client";

/*
 * Properties (DESIGN.md §43, #49): transient context, not a permanent third
 * column — opened with ⌘I / the context menu, closed with ×. General facts,
 * links, and reachable history; restore keeps the e2e locators intact.
 */

export function PropertiesPanel({
  path,
  revision,
  onRestore,
  onClose,
}: {
  path: string | null;
  revision: string | null;
  onRestore: (revision: string) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation("info");
  const [meta, setMeta] = useState<Entry | null>(null);
  const [history, setHistory] = useState<RevisionDto[]>([]);
  const [links, setLinks] = useState<{ outbound: number; backlinks: string[] }>({
    outbound: 0,
    backlinks: [],
  });

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

  return (
    <aside
      className="flex h-full min-h-0 flex-col border-l bg-sidebar"
      data-testid="info-panel"
      aria-label={t("properties")}
    >
      <div className="flex h-11 shrink-0 items-center gap-2 border-b px-3">
        <h2 className="flex-1 text-[13px] font-semibold">{t("properties")}</h2>
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
                  <li
                    key={source}
                    className="truncate font-mono text-[11px] text-muted-foreground"
                    title={source}
                  >
                    {source}
                  </li>
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
                  <span className="shrink-0 font-mono text-[11px] text-muted-foreground">
                    gen {rev.generation}
                  </span>
                  <span className="shrink-0">{rev.action}</span>
                  <span className="min-w-0 flex-1 truncate font-mono text-[10.5px] text-faint">
                    {rev.created_at.slice(0, 19)}
                  </span>
                  {rev.revision === revision ? (
                    <span className="shrink-0 rounded-full bg-primary/10 px-1.5 py-0.5 text-[10px] text-primary">
                      {t("current")}
                    </span>
                  ) : rev.action === "trash" ? (
                    // A tombstone has no content to restore; the trash view's
                    // restore walks history for the newest contentful revision.
                    <span className="shrink-0 text-[10px] text-muted-foreground">
                      {t("tombstone")}
                    </span>
                  ) : (
                    <button
                      data-testid={`restore-revision-gen-${rev.generation}`}
                      onClick={() => onRestore(rev.revision)}
                      className="inline-flex shrink-0 cursor-pointer items-center gap-1 rounded-md px-1.5 py-0.5 text-[10.5px] text-muted-foreground transition-colors hover:bg-primary/10 hover:text-primary focus-visible:outline-2 focus-visible:outline-ring/70"
                    >
                      <ArchiveRestore className="size-3 stroke-[1.5]" />
                      {t("restoreRevision")}
                    </button>
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
