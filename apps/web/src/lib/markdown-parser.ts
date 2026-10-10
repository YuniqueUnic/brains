import { Marked } from "marked";

/** Shared Markdown and wiki-link syntax for rendering and link discovery. */
export function createMarkdownParser(): Marked {
  const renderer = new Marked();
  renderer.use({
    extensions: [
      {
        name: "wikiLink",
        level: "inline",
        start: (source) => source.indexOf("[["),
        tokenizer(source) {
          const match = /^(!?)\[\[([^\]\n]+)\]\]/.exec(source);
          if (!match) return;
          const [href, alias] = match[2].split("|");
          const label = alias ?? href;
          return {
            type: match[1] ? "image" : "link",
            raw: match[0],
            href,
            text: label,
            tokens: [{ type: "text", raw: label, text: label }],
          };
        },
      },
    ],
  });
  return renderer;
}
