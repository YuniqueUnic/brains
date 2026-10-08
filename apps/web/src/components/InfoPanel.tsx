import { useEffect, useState } from "react";
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
    return <aside className="info-panel muted">未选择文件</aside>;
  }

  return (
    <aside className="info-panel" data-testid="info-panel">
      {meta && (
        <section>
          <h3>元信息</h3>
          <dl>
            <dt>generation</dt>
            <dd>{meta.generation}</dd>
            <dt>kind</dt>
            <dd>{meta.kind ?? "-"}</dd>
            {meta.size !== undefined && (
              <>
                <dt>size</dt>
                <dd>{meta.size}</dd>
              </>
            )}
            {meta.updated_at && (
              <>
                <dt>updated_at</dt>
                <dd>{meta.updated_at}</dd>
              </>
            )}
          </dl>
        </section>
      )}
      <section>
        <h3>链接</h3>
        <p className="muted">
          出链 {links.outbound} · 反链 {links.backlinks.length}
        </p>
        <ul className="backlinks">
          {links.backlinks.map((source) => (
            <li key={source}>{source}</li>
          ))}
        </ul>
      </section>
      <section>
        <h3>历史（当前链可达）</h3>
        <ul className="history-list" data-testid="history-list">
          {history.map((rev) => (
            <li key={rev.revision}>
              <span className="muted">gen {rev.generation}</span>{" "}
              <span>{rev.action}</span>{" "}
              <span className="muted">{rev.created_at.slice(0, 19)}</span>
              {rev.revision === revision ? (
                <span className="muted">当前</span>
              ) : rev.action === "trash" ? (
                // A tombstone has no content to restore; the trash view's
                // restore walks history for the newest contentful revision.
                <span className="muted">墓碑（回收站可恢复）</span>
              ) : (
                <button
                  data-testid={`restore-revision-gen-${rev.generation}`}
                  onClick={() => onRestore(rev.revision)}
                >
                  恢复此版本
                </button>
              )}
            </li>
          ))}
        </ul>
      </section>
    </aside>
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
