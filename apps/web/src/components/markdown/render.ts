import DOMPurify from "dompurify";
import { marked } from "marked";

/** All Markdown surfaces share the same renderer and HTML trust boundary. */
export function markdownToSafeHtml(text: string): string {
  return DOMPurify.sanitize(marked.parse(text, { async: false }) as string);
}
