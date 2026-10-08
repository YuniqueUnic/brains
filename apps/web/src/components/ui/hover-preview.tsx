import { useState, type ReactElement } from "react";
import { Tooltip as TooltipPrimitive } from "@base-ui-components/react/tooltip";
import { useTranslation } from "react-i18next";
import type { PreviewResult } from "../properties/preview";

/*
 * HoverPreview (DESIGN.md §49): one control for the "hover a reference →
 * floating rendered preview" pattern — backlinks, history rows, anywhere a
 * path/revision resolves to content. Loader decides what is shown
 * (markdown / image / not-previewable); onOpen makes the popup click-to-jump.
 */
export function HoverPreview({
  trigger,
  load,
  side = "left",
  onOpen,
  testId,
}: {
  /** The element that reveals the preview on hover (keeps its own classes). */
  trigger: ReactElement<Record<string, unknown>>;
  /** Content loader; one-flight caching lives in the loader (preview.ts). */
  load: () => Promise<PreviewResult>;
  side?: "left" | "right" | "top";
  onOpen?: () => void;
  testId?: string;
}) {
  const { t } = useTranslation("info");
  const [preview, setPreview] = useState<PreviewResult | null>(null);
  return (
    <TooltipPrimitive.Root>
      <TooltipPrimitive.Trigger
        render={trigger}
        onMouseEnter={() => {
          void load().then(setPreview, () => setPreview({ kind: "binary" }));
        }}
      />
      <TooltipPrimitive.Portal>
        <TooltipPrimitive.Positioner side={side} sideOffset={10} align="start">
          <TooltipPrimitive.Popup
            className="z-50 max-h-[320px] w-[min(360px,80vw)] overflow-hidden rounded-[10px] border bg-card shadow-overlay"
            role="tooltip"
            data-testid={testId}
            onClick={onOpen}
            style={onOpen ? { cursor: "pointer" } : undefined}
          >
            <div className="flex h-auto max-h-[320px] items-start justify-center overflow-auto">
              {preview === null ? (
                <p className="px-4 py-6 text-xs text-muted-foreground">{t("previewLoading")}</p>
              ) : preview.kind === "image" ? (
                <img
                  src={preview.url}
                  alt=""
                  className="max-h-[320px] w-auto object-contain"
                />
              ) : preview.kind === "text" ? (
                <div
                  className="markdown-preview w-full !py-4 text-[13px]"
                  dangerouslySetInnerHTML={{ __html: preview.html }}
                />
              ) : (
                <p className="px-4 py-6 text-xs text-muted-foreground">
                  {t("previewUnavailable")}
                </p>
              )}
            </div>
          </TooltipPrimitive.Popup>
        </TooltipPrimitive.Positioner>
      </TooltipPrimitive.Portal>
    </TooltipPrimitive.Root>
  );
}
