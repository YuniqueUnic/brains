import { api, type RevisionDto } from "../../api/client";

/*
 * Tombstones carry no content: both restore and the trash viewer walk the
 * history chain for the newest contentful revision (design §8.1 — restore
 * takes a live source). One walk returns the revision AND the anchor the
 * conditional restore needs.
 */
export async function newestContentful(
  path: string,
  maxPages = 10,
): Promise<{ revision: string; anchor: string; content: RevisionDto["content"] } | null> {
  const anchor = (await api.history(path)).anchor_revision;
  let cursor: string | null = null;
  for (let page = 0; page < maxPages; page += 1) {
    const result = await api.history(path, cursor ?? undefined);
    const contentful = result.items.find((rev) => rev.action !== "trash");
    if (contentful) return { revision: contentful.revision, anchor, content: contentful.content };
    cursor = result.next_cursor;
    if (!cursor) break;
  }
  return null;
}
