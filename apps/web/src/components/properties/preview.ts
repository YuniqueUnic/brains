import { api, isTextMedia } from "../../api/client";

/*
 * Shared hover-preview loaders (DESIGN.md §49): backlinks, history rows and
 * editor reference hovers load the same three outcomes — source text,
 * a blob image, or "not previewable". MarkdownContent owns rendering and
 * sanitizing the source text. One-flight caches so
 * repeated hovers don't refetch.
 */

export type FilePreview =
  | { kind: "text"; text: string }
  | { kind: "image"; url: string }
  | { kind: "binary" };

export type PreviewResult = FilePreview | { kind: "link"; url: string };

const previewCache = new Map<string, Promise<FilePreview>>();

/** Live (HEAD) content of a path. */
export function previewFor(path: string): Promise<FilePreview> {
  let cached = previewCache.get(path);
  if (!cached) {
    cached = (async () => {
      const meta = await api.readMeta(path);
      if (meta.media_type?.startsWith("image/")) {
        const { bytes } = await api.readRaw(path);
        const blob = new Blob([bytes as unknown as BlobPart], { type: meta.media_type });
        return { kind: "image", url: URL.createObjectURL(blob) } as const;
      }
      if (isTextMedia(meta.media_type)) {
        const { text } = await api.readText(path);
        return { kind: "text", text } as const;
      }
      return { kind: "binary" } as const;
    })();
    // A failed preview must not be sticky: drop the entry so the next hover
    // retries (the file may have appeared or the error been transient).
    cached = cached.catch((error) => {
      previewCache.delete(path);
      throw error;
    });
    previewCache.set(path, cached);
  }
  return cached;
}

const revisionCache = new Map<string, Promise<FilePreview>>();

/** Called after a publish: cached HEAD/revision previews for the path are
 * stale the moment new bytes land. */
export function invalidatePreview(path: string) {
  previewCache.delete(path);
  for (const key of [...revisionCache.keys()]) {
    if (key.startsWith(`${path}@`)) revisionCache.delete(key);
  }
}

/** Content of a specific historical revision (history hover / view dialog). */
export function revisionPreviewFor(path: string, revision: string, mediaType = "text/markdown"): Promise<FilePreview> {
  const key = `${path}@${revision}`;
  let cached = revisionCache.get(key);
  if (!cached) {
    cached = (async () => {
      if (mediaType.startsWith("image/")) {
        const bytes = await api.readRevisionRaw(path, revision);
        const blob = new Blob([bytes as unknown as BlobPart], { type: mediaType });
        return { kind: "image", url: URL.createObjectURL(blob) } as const;
      }
      if (!isTextMedia(mediaType)) return { kind: "binary" } as const;
      const text = await api.readRevisionText(path, revision);
      return { kind: "text", text } as const;
    })();
    cached = cached.catch((error) => {
      revisionCache.delete(key);
      throw error;
    });
    revisionCache.set(key, cached);
  }
  return cached;
}
