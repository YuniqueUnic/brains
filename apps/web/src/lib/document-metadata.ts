import { parseDocument, isMap, isScalar, isSeq } from "yaml";

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
  /** UTF-16 source ranges, shared by editor decoration and hover lookup. */
  frontmatter?: {
    end: number;
    tags: { from: number; to: number; tag: string }[];
  };
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
    const acceptedTags = tags.slice(0, 64);
    const tagNode = doc.get("tags", true);
    const nodes = isSeq(tagNode) ? tagNode.items : [tagNode];
    const offset = match[0].indexOf("\n") + 1;
    return {
      body: source.slice(match[0].length),
      invalidMetadata: false,
      frontmatter: {
        end: match[0].length,
        tags: nodes.flatMap((node) => {
          if (!isScalar(node) || !node.range) return [];
          const tag = string(node.value);
          return tag && acceptedTags.includes(tag)
            ? [{ from: offset + node.range[0], to: offset + node.range[1], tag }]
            : [];
        }),
      },
      metadata: {
        title: string(values.title),
        description: string(values.description),
        tags: acceptedTags,
        tagsTruncated: tags.length > 64,
        links,
      },
    };
  } catch {
    return { body: source, metadata: null, invalidMetadata: true };
  }
}
