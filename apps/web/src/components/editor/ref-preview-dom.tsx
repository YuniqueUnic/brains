import { createRoot } from "react-dom/client";
import { previewFor } from "../properties/preview";
import { previewCorner } from "../ui/preview-resize";
import { PreviewSurface } from "../ui/preview-surface";
import type { MarkdownContext } from "../markdown/markdown-content";
import type { Reference } from "../markdown/references";

/** CodeMirror owns the shell lifecycle; React owns shared interactive content. */
export function buildRefPreviewDom(
  reference: Reference,
  context: MarkdownContext,
  options: {
    image?: boolean;
    completion?: boolean;
    anchor?: () => {
      left: number;
      right: number;
      top: number;
      bottom: number;
    } | null;
    onResize?: () => void;
  } = {},
) {
  const dom = document.createElement("div");
  dom.className = "brains-ref-hover";
  dom.dataset.resolved = reference.kind === "vault" ? reference.path : "";
  const root = createRoot(dom);
  const anchor = () =>
    options.completion
      ? dom.parentElement?.parentElement
          ?.querySelector('[aria-selected="true"]')
          ?.getBoundingClientRect()
      : options.anchor?.();
  const load = () =>
    reference.kind === "vault"
      ? previewFor(reference.path)
      : Promise.resolve(
          reference.kind === "external"
            ? {
                kind: options.image ? ("image" as const) : ("link" as const),
                url: reference.url,
              }
            : { kind: "binary" as const },
        );
  root.render(
    <PreviewSurface
      frame={() => ({
        element: dom.parentElement!,
        styleOwner: options.completion
          ? dom.parentElement!.parentElement!
          : dom.parentElement!,
      })}
      load={load}
      context={{
        ...context,
        path: reference.kind === "vault" ? reference.path : context.path,
      }}
      title={
        reference.kind === "external"
          ? reference.url
          : reference.kind === "missing"
            ? reference.target
            : reference.kind === "anchor"
              ? reference.fragment
              : reference.path
      }
      onOpen={
        reference.kind === "vault" && context.onOpen
          ? () => context.onOpen!(reference.path)
          : undefined
      }
      anchor={() => anchor() ?? undefined}
      onResize={options.onResize}
    />,
  );
  return {
    dom,
    update() {
      dom
        .querySelector(".preview-surface")
        ?.dispatchEvent(new Event("preview-reset-placement"));
    },
    positioned() {
      const surface = dom.querySelector<HTMLElement>(".preview-surface");
      const handle = dom.querySelector<HTMLElement>(".preview-resize");
      if (
        surface &&
        handle &&
        dom.parentElement?.dataset.previewPlacement !== "manual"
      )
        handle.dataset.corner = previewCorner(
          surface.getBoundingClientRect(),
          anchor() ?? undefined,
        );
    },
    destroy() {
      root.unmount();
    },
  };
}
