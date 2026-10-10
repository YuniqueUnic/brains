import { useRef, type ReactElement } from "react";
import { Popover } from "@base-ui-components/react/popover";
import type { PreviewResult } from "../properties/preview";
import type { MarkdownContext } from "../markdown/markdown-content";
import { PreviewSurface } from "./preview-surface";

/** Interactive previews use a popover; tooltips must not contain controls. */
export function HoverPreview({
  trigger,
  load,
  side = "left",
  onOpen,
  testId,
  context,
}: {
  trigger: ReactElement<Record<string, unknown>>;
  load: () => Promise<PreviewResult>;
  side?: "left" | "right" | "top" | "bottom";
  onOpen?: () => void;
  testId?: string;
  context?: MarkdownContext;
}) {
  const triggerRef = useRef<HTMLElement>(null);
  const positionerRef = useRef<HTMLDivElement>(null);
  return (
    <Popover.Root>
      <Popover.Trigger
        ref={triggerRef}
        render={trigger}
        nativeButton={trigger.type === "button"}
        role={trigger.type === "a" ? "link" : undefined}
        openOnHover
        delay={350}
        closeDelay={300}
      />
      <Popover.Portal>
        <Popover.Positioner
          ref={positionerRef}
          positionMethod="fixed"
          side={side}
          sideOffset={10}
          align="start"
          className="z-50"
        >
          <Popover.Popup
            className="preview-popup"
            data-testid={testId}
            initialFocus={false}
            finalFocus={false}
          >
            <PreviewSurface
              frame={() => ({
                element: positionerRef.current!,
                styleOwner: positionerRef.current!,
              })}
              load={load}
              onOpen={onOpen}
              context={context}
              anchor={() => triggerRef.current?.getBoundingClientRect()}
            />
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  );
}
