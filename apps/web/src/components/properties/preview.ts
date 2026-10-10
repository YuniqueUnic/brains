import { markdownToSafeHtml } from "../markdown/render";
import { api, isTextMedia } from "../../api/client";

/*
 * Shared hover-preview loaders (DESIGN.md §49): backlinks, history rows and
 * editor reference hovers all render the same three outcomes — sanitized
 * markdown, a blob image, or "not previewable". One-flight caches so
 * repeated hovers don't refetch.
 */

export type PreviewResult =
  | { kind: "text"; html: string }
  | { kind: "image"; url: string }
  | { kind: "binary" };

const previewCache = new Map<string, Promise<PreviewResult>>();

/** Live (HEAD) content of a path. */
export function previewFor(path: string): Promise<PreviewResult> {
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
        return { kind: "text", html: await markdownToSafeHtml(text) } as const;
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

const revisionCache = new Map<string, Promise<PreviewResult>>();

/** Called after a publish: cached HEAD/revision previews for the path are
 * stale the moment new bytes land. */
export function invalidatePreview(path: string) {
  previewCache.delete(path);
  for (const key of [...revisionCache.keys()]) {
    if (key.startsWith(`${path}@`)) revisionCache.delete(key);
  }
}

/** Content of a specific historical revision (history hover / view dialog). */
export function revisionPreviewFor(path: string, revision: string): Promise<PreviewResult> {
  const key = `${path}@${revision}`;
  let cached = revisionCache.get(key);
  if (!cached) {
    cached = (async () => {
      const meta = await api.readMeta(path).catch(() => null);
      const mediaType = meta?.media_type ?? "text/markdown";
      if (mediaType.startsWith("image/")) {
        const bytes = await api.readRevisionRaw(path, revision);
        const blob = new Blob([bytes as unknown as BlobPart], { type: mediaType });
        return { kind: "image", url: URL.createObjectURL(blob) } as const;
      }
      const text = await api.readRevisionText(path, revision);
      return { kind: "text", html: await markdownToSafeHtml(text) } as const;
    })();
    cached = cached.catch((error) => {
      revisionCache.delete(key);
      throw error;
    });
    revisionCache.set(key, cached);
  }
  return cached;
}

/**
 * Resolve a markdown/wiki reference to a vault path: exact, then any entry
 * ending with `/<target>`, then stem match ([[name]] without extension).
 */
export const referenceStem = (path: string) => path.replace(/\.[a-z0-9]+$/i, "");

export function resolveReference(
  target: string,
  files: { path: string }[],
): string | null {
  let decoded = target;
  try {
    decoded = decodeURIComponent(target);
  } catch {
    /* keep raw */
  }
  if (files.some((file) => file.path === decoded)) return decoded;
  const suffixed = files.find((file) => file.path.endsWith(`/${decoded}`));
  if (suffixed) return suffixed.path;
  const stem = referenceStem(decoded);
  const byStem = files.find(
    (file) => referenceStem(file.path) === stem || referenceStem(file.path).endsWith(`/${stem}`),
  );
  return byStem?.path ?? null;
}
