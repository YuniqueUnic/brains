import DOMPurify from "dompurify";
import { createMarkdownParser } from "../../lib/markdown-parser";
import { resolveReference } from "./references";

/** All Markdown surfaces share the same renderer and HTML trust boundary. */
export function markdownToSafeHtml(
  text: string,
  context?: { path: string; files: readonly { path: string }[] },
): string {
  const renderer = createMarkdownParser();
  const paths = new Map<string, string>();
  const images = new Map<string, string>();
  const headings = new Map<string, number>();
  renderer.use({
    renderer: {
      heading(token) {
        const base = token.text
          .toLowerCase()
          .replace(/<[^>]*>/g, "")
          .replace(/[^\p{L}\p{N} _-]/gu, "")
          .trim()
          .replace(/\s+/g, "-");
        const count = headings.get(base) ?? 0;
        headings.set(base, count + 1);
        const id = count ? `${base}-${count}` : base;
        return `<h${token.depth} id="${id}">${this.parser.parseInline(token.tokens)}</h${token.depth}>`;
      },
    },
  });
  if (context)
    renderer.use({
      walkTokens(token) {
        if (token.type !== "link" && token.type !== "image") return;
        const ref = resolveReference(token.href, context.files, context.path);
        if (ref.kind !== "vault") return;
        if (token.type === "image") {
          token.href = `#vault-image-${images.size}`;
          images.set(token.href, ref.path);
        } else {
          token.href = `#${encodeURIComponent(ref.path)}${ref.fragment}`;
          paths.set(token.href, ref.path);
        }
      },
    });
  const html = DOMPurify.sanitize(
    renderer.parse(text, { async: false }) as string,
  );
  const template = document.createElement("template");
  template.innerHTML = html;
  for (const anchor of template.content.querySelectorAll("a")) {
    const path = paths.get(anchor.getAttribute("href") ?? "");
    if (path) anchor.dataset.vaultPath = path;
    else if (/^https?:/i.test(anchor.getAttribute("href") ?? "")) {
      anchor.target = "_blank";
      anchor.rel = "noopener noreferrer";
    }
  }
  for (const image of template.content.querySelectorAll("img")) {
    const path = images.get(image.getAttribute("src") ?? "");
    if (path) {
      image.dataset.vaultImage = path;
      image.removeAttribute("src");
    }
  }
  return template.innerHTML;
}
