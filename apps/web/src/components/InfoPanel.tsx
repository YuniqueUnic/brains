import { useEffect, useState } from "react";
import { History, Link2, ListTree, ArchiveRestore, Hash } from "lucide-react";
import { useTranslation } from "react-i18next";
import { api, type Entry, type RevisionDto } from "../api/client";

/** Right-hand metadata panel: entry facts, links and reachable history. */
export default function InfoPanel({
  path,
  revision,
  onRestore,
}: {
  path: string | null;
  revision: string | null;
  onRestore: (revision: string) => void;
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

  if (!path) {
    return (
      <aside className="flex h-full flex-col items-center justify-center gap-2 text-muted-foreground">
        <ListTree className="size-5 opacity-60" />
        <p className="text-xs">{t("noSelection")}</p>
      </aside>
    );
  }

  return (
    <aside className="flex h-full min-h-0 flex-col gap-2 overflow-auto" data-testid="info-panel">
      {meta && (
        <Section icon={<Hash className="size-3.5" />} title={t("meta")}>
          <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-[11.5px]">
            <Fact label="generation" value={String(meta.generation)} mono />
            <Fact label="kind" value={meta.kind ?? "-"} mono />
            {meta.size !== undefined && <Fact label="size" value={String(meta.size)} mono />}
            {meta.updated_at && <Fact label="updated_at" value={meta.updated_at} mono />}
          </dl>
        </Section>
      )}

      <Section icon={<Link2 className="size-3.5" />} title="链接">
        <p className="text-[11.5px] text-muted-foreground">
          {t("linkSummary", { outbound: links.outbound, backlinks: links.backlinks.length })}
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
      </Section>

      <Section icon={<History className="size-3.5" />} title="历史（当前链可达）">
        <ul className="space-y-1" data-testid="history-list">
          {history.map((rev) => (
            <li
              key={rev.revision}
              className="flex items-center gap-2 rounded-md px-1.5 py-1 text-[11.5px] hover:bg-muted/70"
            >
              <span className="shrink-0 font-mono text-muted-foreground">
                gen {rev.generation}
              </span>
              <span className="shrink-0">{rev.action}</span>
              <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-muted-foreground">
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
                  className="inline-flex shrink-0 items-center gap-1 rounded-md px-1.5 py-0.5 text-[10.5px] text-muted-foreground transition-colors hover:bg-primary/10 hover:text-primary focus-visible:outline-2 focus-visible:outline-ring"
                >
                  <ArchiveRestore className="size-3" />
                  {t("restoreRevision")}
                </button>
              )}
            </li>
          ))}
          {history.length === 0 && (
            <li className="px-1.5 text-[11px] text-muted-foreground">{t("noHistory")}</li>
          )}
        </ul>
      </Section>
    </aside>
  );
}

function Section({
  icon,
  title,
  children,
}: {
  icon: React.ReactNode;
  title: string;
  children: React.ReactNode;
}) {
  return (
    <section className="rounded-lg border bg-card p-2.5 shadow-card">
      <h3 className="mb-1.5 flex items-center gap-1.5 text-xs font-semibold text-foreground">
        <span className="text-muted-foreground">{icon}</span>
        {title}
      </h3>
      {children}
    </section>
  );
}

function Fact({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <>
      <dt className="text-[11px] text-muted-foreground">{label}</dt>
      <dd className={mono ? "font-mono text-[11px]" : "text-[12px]"}>{value}</dd>
    </>
  );
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
