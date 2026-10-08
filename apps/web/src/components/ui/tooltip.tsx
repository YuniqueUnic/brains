import { Tooltip as TooltipPrimitive } from "@base-ui-components/react/tooltip";
import { cn } from "../../lib/utils";

/** Base UI tooltip styled to the token system; hides when content is empty. */
export function Tooltip({
  content,
  children,
  side = "bottom",
}: {
  content: string;
  children: React.ReactElement<Record<string, unknown>>;
  side?: "top" | "bottom" | "left" | "right";
}) {
  if (!content) return children;
  return (
    <TooltipPrimitive.Root>
      <TooltipPrimitive.Trigger render={children} />
      <TooltipPrimitive.Portal>
        <TooltipPrimitive.Positioner side={side} sideOffset={6}>
          <TooltipPrimitive.Popup
            className={cn(
              "z-50 max-w-[min(22rem,calc(100vw-2rem))] rounded-md border bg-card px-2.5 py-1.5",
              "text-xs text-foreground shadow-menu",
            )}
          >
            {content}
          </TooltipPrimitive.Popup>
        </TooltipPrimitive.Positioner>
      </TooltipPrimitive.Portal>
    </TooltipPrimitive.Root>
  );
}
