import { parseDocument, isMap } from "yaml";

export interface DocumentMetadata {
  title?: string;
  description?: string;
  tags: string[];
  tagsTruncated: boolean;
  links: { label: string; target: string }[];
}
export interface MarkdownDocument {
  body: string;
  metadata: DocumentMetadata | null;
  invalidMetadata: boolean;
}

const string = (value: unknown): string | undefined =>
  typeof value === "string" && value.trim() ? value.trim() : undefined;
const strings = (value: unknown): string[] =>
  (Array.isArray(value) ? value : [value]).flatMap((item) =>
    string(item) ? [string(item)!] : [],
  );

/** Metadata is derived from the current document, never a second persisted model. */
export function parseMarkdownDocument(source: string): MarkdownDocument {
  const match = /^\uFEFF?---\r?\n([\s\S]*?)\r?\n(?:---|\.\.\.)(?:\r?\n|$)/.exec(
    source,
  );
  if (!match) return { body: source, metadata: null, invalidMetadata: false };
  try {
    // Bound untrusted frontmatter and alias expansion before creating JS values.
    if (new TextEncoder().encode(match[1]).length > 16 * 1024)
      throw new Error("metadata too large");
    const doc = parseDocument(match[1], { schema: "core" });
    if (doc.errors.length || !isMap(doc.contents))
      throw new Error("invalid metadata");
    const values = doc.toJS({ maxAliasCount: 32 }) as Record<string, unknown>;
    const links: DocumentMetadata["links"] = [];
    const add = (value: unknown, label?: unknown) => {
      const href = string(value);
      if (!href) return;
      const wiki = /^\[\[([^\]]+)\]\]$/.exec(href);
      const [target, alias] = wiki ? wiki[1].split("|") : [href];
      links.push({ target, label: string(label) ?? alias ?? target });
    };
    if (Array.isArray(values.links)) {
      for (const link of values.links) {
        if (typeof link === "string") add(link);
        else if (link && typeof link === "object")
          add(link.url, link.title ?? link.label);
      }
    } else if (values.links && typeof values.links === "object") {
      for (const [label, target] of Object.entries(values.links))
        add(target, label);
    } else add(values.links);
    const tags = [...new Set(strings(values.tags))];
    return {
      body: source.slice(match[0].length),
      invalidMetadata: false,
      metadata: {
        title: string(values.title),
        description: string(values.description),
        tags: tags.slice(0, 64),
        tagsTruncated: tags.length > 64,
        links,
      },
    };
  } catch {
    return { body: source, metadata: null, invalidMetadata: true };
  }
}
