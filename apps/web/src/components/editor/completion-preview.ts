import type { Extension } from "@codemirror/state";
import { ViewPlugin, type PluginValue } from "@codemirror/view";
import { placeBesideHorizontally } from "../../lib/fit-viewport";
import { buildRefPreviewDom, type RefPreviewLabels } from "./ref-preview-dom";

/*
 * Completion-list preview (HoverPreview 同款): while the [[wiki / []() link
 * autocomplete tooltip is open, a rendered file preview floats beside it and
 * follows the highlighted option — arrow keys and mouse both move the list's
 * aria-selected, so one observer covers both. The popup mounts on <body>
 * next to the tooltip (inverse-zoom layer), is positioned by pure
 * viewport-fitting math (right of the list, flipping left when out of
 * room), and is click-to-open like every other reference preview.
 */
export function completionPreview(
  onOpen: ((path: string) => void) | undefined,
  labels: RefPreviewLabels,
): Extension {
  return ViewPlugin.fromClass(
    class implements PluginValue {
      private host = document.createElement("div");
      private card = document.createElement("div");
      private currentPath: string | null = null;
      /** The autocomplete list we currently mirror; null when it is closed. */
      private list: Element | null = null;
      private bodyObserver = new MutationObserver(() => this.sync());
      private listObserver: MutationObserver | null = null;

      constructor() {
        this.host.className = "cm-completion-preview-host";
        this.card.className = "cm-completion-preview-card";
        this.card.style.display = "none";
        this.host.append(this.card);
        document.body.append(this.host);
        this.bodyObserver.observe(document.body, { childList: true, subtree: true });
      }

      destroy() {
        this.bodyObserver.disconnect();
        this.listObserver?.disconnect();
        this.host.remove();
      }

      private sync() {
        const list = document.querySelector(".cm-tooltip-autocomplete ul");
        if (!list) {
          this.detach();
          return;
        }
        if (list !== this.list) {
          this.listObserver?.disconnect();
          this.listObserver = new MutationObserver(() => this.sync());
          this.listObserver.observe(list, {
            attributes: true,
            attributeFilter: ["aria-selected"],
            childList: true,
            subtree: true,
          });
          this.list = list;
        }
        const detail = list
          .querySelector('li[aria-selected="true"] .cm-completionDetail')
          ?.textContent?.trim();
        if (!detail) {
          this.currentPath = null;
          this.card.style.display = "none";
          return;
        }
        if (detail !== this.currentPath) {
          this.currentPath = detail;
          this.card.textContent = "";
          this.card.style.display = "";
          this.card.append(buildRefPreviewDom(detail, onOpen, labels));
        }
        this.place();
      }

      private detach() {
        this.listObserver?.disconnect();
        this.listObserver = null;
        this.list = null;
        this.currentPath = null;
        this.card.style.display = "none";
      }

      /** Right of the list (flip left when out of room), never off-viewport. */
      private place() {
        const tooltip = this.list?.closest(".cm-tooltip");
        if (!tooltip) return;
        const anchor = tooltip.getBoundingClientRect();
        const size = this.card.getBoundingClientRect();
        if (anchor.width === 0 || size.width === 0) return;
        const fit = placeBesideHorizontally(
          { x: anchor.x, y: anchor.y, width: anchor.width, height: anchor.height },
          { width: size.width, height: size.height },
          { width: window.innerWidth, height: window.innerHeight },
        );
        this.card.style.transform = `translate(${fit.x}px, ${fit.y}px)`;
      }
    },
  );
}
