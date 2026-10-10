/** Whitespace commits #tags; Enter/Tab also commits the unfinished final tag. */
export function extractTagQuery(value: string, finish = false) {
  const tags: string[] = [];
  const query = value
    .replace(
      /(^|\s)#([^\s#]+)(?=\s|$)/g,
      (token, space: string, tag: string, offset: number) => {
        if (!finish && offset + token.length === value.length) return token;
        tags.push(tag);
        return space;
      },
    )
    .replace(/^\s+/, "");
  return { query, tags };
}
