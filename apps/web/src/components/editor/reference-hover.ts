import type { Extension } from "@codemirror/state";
import { hoverTooltip } from "@codemirror/view";
import { previewFor, resolveReference } from "../properties/preview";
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
        create: () => ({ dom: buildHoverDom(resolved, onOpen, labels) }),
      };
    },
    { hoverTime: 350 },
  );
}

function buildHoverDom(
  resolved: string,
  onOpen: ((path: string) => void) | undefined,
  labels: { loading: string; unavailable: string },
): HTMLElement {
  const dom = document.createElement("div");
  dom.className = "brains-ref-hover";
  const fill = (body: () => HTMLElement) => {
    const content = document.createElement("div");
    content.className = "brains-ref-hover-body";
    content.append(body());
    dom.append(content);
    if (onOpen) {
      content.addEventListener("click", () => onOpen(resolved));
      content.setAttribute("title", resolved);
    }
  };
  const note = (text: string) => () => {
    const p = document.createElement("p");
    p.className = "brains-ref-hover-note";
    p.textContent = text;
    return p;
  };
  dom.dataset.resolved = resolved;
  dom.append(Object.assign(document.createElement("p"), {
    className: "brains-ref-hover-note",
    textContent: labels.loading,
  }));
  void previewFor(resolved).then(
    (preview) => {
      dom.textContent = "";
      const head = document.createElement("p");
      head.className = "brains-ref-hover-path";
      head.textContent = resolved;
      dom.append(head);
      if (preview.kind === "image") {
        fill(() => {
          const img = document.createElement("img");
          img.src = preview.url;
          img.alt = resolved;
          return img;
        });
      } else if (preview.kind === "text") {
        fill(() => {
          const div = document.createElement("div");
          div.className = "markdown-preview brains-ref-hover-md";
          div.innerHTML = preview.html;
          return div;
        });
      } else {
        fill(note(labels.unavailable));
      }
    },
    () => {
      dom.textContent = "";
      fill(note(labels.unavailable));
    },
  );
  return dom;
}
