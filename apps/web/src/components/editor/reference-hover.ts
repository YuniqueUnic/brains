import type { Extension } from "@codemirror/state";
import { hoverTooltip } from "@codemirror/view";
import { resolveReference } from "../properties/preview";
import { buildRefPreviewDom } from "./ref-preview-dom";
import type { CompletionFile } from "./completions";

/*
 * Hover preview for vault references (DESIGN.md §41, inkmark link hover):
 * pointing at a `[](/![](target)` URL or a `[[wiki]]` name that resolves to
 * a vault file pops the same rendered preview the properties panel uses;
 * clicking it opens the file. Non-vault URLs resolve to nothing → no popup.
 */
export function referenceHover(
  files: CompletionFile[],
  onOpen: ((path: string) => void) | undefined,
  labels: { loading: string; unavailable: string },
): Extension {
  return hoverTooltip(
    (view, pos) => {
      const line = view.state.doc.lineAt(pos);
      const text = line.text;
      const offset = pos - line.from;
      const ranges: { from: number; to: number; target: string }[] = [];
      for (const match of text.matchAll(/\[[^\]]*\]\(([^()\s]+)\)/g)) {
        const start = (match.index ?? 0) + match[0].indexOf("](") + 2;
        ranges.push({ from: start, to: start + match[1].length, target: match[1] });
      }
      for (const match of text.matchAll(/\[\[([^\]]+)\]\]/g)) {
        const start = (match.index ?? 0) + 2;
        ranges.push({ from: start, to: start + match[1].length, target: match[1] });
      }
      const hit = ranges.find((range) => offset >= range.from && offset <= range.to);
      if (!hit) return null;
      const resolved = resolveReference(hit.target, files);
      if (!resolved) return null;
      return {
        pos: line.from + hit.from,
        end: line.from + hit.to,
        above: true,
        create: () => ({ dom: buildRefPreviewDom(resolved, onOpen, labels) }),
      };
    },
    { hoverTime: 350 },
  );
}
