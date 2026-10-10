export type SearchMode = "literal" | "fts";

/** Only complete, standalone tokens change filters. The last mode wins. */
export function extractSearchQuery(value: string, finish = false) {
  const tags: string[] = [];
  let mode: SearchMode | undefined;
  let pendingToken = false;
  const query = value
    .replace(
      /(^|\s)(?:#([^\s#]+)|\/(fts|text|literal))(?=\s|$)/g,
      (token, space: string, tag: string | undefined, command: string | undefined, offset: number) => {
        if (!finish && offset + token.length === value.length) {
          pendingToken = true;
          return token;
        }
        if (tag) tags.push(tag);
        else if (command) mode = command === "fts" ? "fts" : "literal";
        return space;
      },
    )
    .replace(/^\s+/, "");
  return { query, tags, mode, pendingToken };
}
