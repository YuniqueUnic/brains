import { previewFor } from "../properties/preview";
import { fitIntoViewport } from "../../lib/fit-viewport";

/*
 * Shared vanilla-DOM preview popup (DESIGN.md §41, HoverPreview 同款):
 * one builder for "resolve a vault path → floating rendered preview" used by
 * the editor reference hover AND the completion-list preview. Async content
 * can outgrow the spot the anchor picked: every fill re-clamps into the
 * viewport (data-dx/dy accumulate, translate survives zoom layers).
 */

export interface RefPreviewLabels {
  loading: string;
  unavailable: string;
}

/**
 * Build the floating preview card. New call sites MUST pick a side: a
 * clipped, scrollable container → selfClamp:false (the card owns
 * positioning, e.g. the completion preview); anything else → true.
 */
export function buildRefPreviewDom(
  resolved: string,
  onOpen: ((path: string) => void) | undefined,
  labels: RefPreviewLabels,
  /** false when the caller owns positioning inside its own clipped,
   * scrollable card (completion preview) — a self-translating child would
   * slide content out of that card's top edge. */
  { selfClamp = true }: { selfClamp?: boolean } = {},
): HTMLElement {
  const dom = document.createElement("div");
  dom.className = "brains-ref-hover";
  // Position corrections are visual px; inside the inverse-zoomed portal
  // layer translate is scaled, so accumulated deltas are stored to re-derive
  // the local-space correction on every clamp pass.
  const clampIntoViewport = () => {
    const rect = dom.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) return;
    // Huge documents cap at the viewport: a 10k-px child translated to "fit"
    // would slide its own header out of any clipped container.
    if (rect.height > window.innerHeight - 16) {
      dom.style.maxHeight = `${window.innerHeight - 16}px`;
    }
    const fit = fitIntoViewport(
      { x: rect.x, y: rect.y, width: rect.width, height: rect.height },
      { width: window.innerWidth, height: window.innerHeight },
      8,
    );
    const dx = fit.x - rect.x;
    const dy = fit.y - rect.y;
    if (dx !== 0 || dy !== 0) {
      const accX = (Number(dom.dataset.dx) || 0) + dx;
      const accY = (Number(dom.dataset.dy) || 0) + dy;
      dom.style.translate = `${accX}px ${accY}px`;
      dom.dataset.dx = String(accX);
      dom.dataset.dy = String(accY);
    }
  };
  const clamp = () => {
    if (selfClamp) clampIntoViewport();
  };
  const fill = (body: () => HTMLElement) => {
    const content = document.createElement("div");
    content.className = "brains-ref-hover-body";
    content.append(body());
    dom.append(content);
    if (onOpen) {
      content.addEventListener("click", () => onOpen(resolved));
      content.setAttribute("title", resolved);
    }
    clamp();
  };
  const note = (text: string) => () => {
    const p = document.createElement("p");
    p.className = "brains-ref-hover-note";
    p.textContent = text;
    return p;
  };
  dom.dataset.resolved = resolved;
  dom.append(
    Object.assign(document.createElement("p"), {
      className: "brains-ref-hover-note",
      textContent: labels.loading,
    }),
  );
  clamp();
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
