import type { Extension } from "@codemirror/state";
import { syntaxTree } from "@codemirror/language";
import { hoverTooltip, repositionTooltips } from "@codemirror/view";
import { Lexer } from "marked";
import { resolveReference } from "../markdown/references";
import type { MarkdownContext } from "../markdown/markdown-content";
import { buildRefPreviewDom } from "./ref-preview-dom";

/** Lezer owns Markdown ranges; marked owns destination/definition semantics. */
export function referenceHover(context: MarkdownContext): Extension {
  return hoverTooltip(
    (view, pos, side) => {
      let inCode = false;
      let reference:
        | { from: number; to: number; target: string; image: boolean }
        | undefined;
      for (
        let node = syntaxTree(view.state).resolveInner(pos, side);
        node;
        node = node.parent!
      ) {
        if (["CodeBlock", "FencedCode"].includes(node.name)) return null;
        if (node.name === "InlineCode") inCode = true;
        if (["Link", "Image", "Autolink"].includes(node.name)) {
          const lexer = new Lexer();
          lexer.lex(view.state.doc.toString());
          const token = lexer.inlineTokens(
            view.state.sliceDoc(node.from, node.to),
          )[0];
          if (token?.type === "link" || token?.type === "image") {
            reference = {
              from: node.from,
              to: node.to,
              target: token.href,
              image: token.type === "image",
            };
            break;
          }
        }
      }
      if (!reference && !inCode) {
        const line = view.state.doc.lineAt(pos);
        for (const match of line.text.matchAll(/!?\[\[([^\]\n]+)\]\]/g)) {
          const from = line.from + match.index!;
          if (pos >= from && pos <= from + match[0].length) {
            reference = {
              from,
              to: from + match[0].length,
              target: match[1].split("|")[0],
              image: match[0].startsWith("!"),
            };
            break;
          }
        }
      }
      if (!reference) return null;
      const { from, to, target, image } = reference;
      const resolved = resolveReference(
        target,
        context.files ?? [],
        context.path,
      );
      if (resolved.kind === "anchor") return null;
      let anchor = view.coordsAtPos(from);
      return {
        pos: from,
        end: to,
        above: true,
        create: () => ({
          ...buildRefPreviewDom(resolved, context, {
            image,
            anchor: () => anchor,
            onResize: () => repositionTooltips(view),
          }),
          getCoords(pos) {
            anchor = view.coordsAtPos(pos);
            return anchor ?? { left: 0, right: 0, top: -1e6, bottom: -1e6 };
          },
        }),
      };
    },
    { hoverTime: 350 },
  );
}
