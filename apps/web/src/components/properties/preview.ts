import DOMPurify from "dompurify";
import { marked } from "marked";
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

export async function markdownToSafeHtml(text: string): Promise<string> {
  return DOMPurify.sanitize(marked.parse(text, { async: false }) as string);
}

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
    previewCache.set(path, cached);
  }
  return cached;
}

const revisionCache = new Map<string, Promise<PreviewResult>>();

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
    revisionCache.set(key, cached);
  }
  return cached;
}

/**
 * Resolve a markdown/wiki reference to a vault path: exact, then any entry
 * ending with `/<target>`, then stem match ([[name]] without extension).
 */
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
  const stem = decoded.replace(/\.[a-z0-9]+$/i, "");
  const byStem = files.find(
    (file) => file.path === stem || file.path.endsWith(`/${stem}`),
  );
  return byStem?.path ?? null;
}
