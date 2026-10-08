import { Dialog as DialogPrimitive } from "@base-ui-components/react/dialog";
import { X } from "lucide-react";
import type { ComponentProps, ReactNode } from "react";
import { cn } from "../../lib/utils";

/*
 * Dialog on Base UI (DESIGN.UI.md §1): the project owns the visuals; the
 * primitive owns focus trapping, scroll lock and dismiss semantics.
 * Controlled usage: <Dialog open onOpenChange><DialogContent …>.
 */
export const Dialog = DialogPrimitive.Root;
export const DialogClose = DialogPrimitive.Close;

export function DialogContent({
  className,
  children,
  ...props
}: ComponentProps<typeof DialogPrimitive.Popup> & { children: ReactNode }) {
  return (
    <DialogPrimitive.Portal>
      <DialogPrimitive.Backdrop className="fixed inset-0 z-40 bg-black/35 backdrop-blur-[2px] fade-in" />
      <DialogPrimitive.Popup
        className={cn(
          "fixed left-1/2 top-1/2 z-50 w-[min(28rem,94vw)] -translate-x-1/2 -translate-y-1/2",
          "rounded-[12px] border bg-card p-5 text-foreground shadow-overlay overlay-in",
          className,
        )}
        {...props}
      >
        {children}
      </DialogPrimitive.Popup>
    </DialogPrimitive.Portal>
  );
}

export function DialogHeader({ className, ...props }: ComponentProps<"div">) {
  return <div className={cn("flex flex-col gap-1", className)} {...props} />;
}

export function DialogTitle({ className, ...props }: ComponentProps<"h2">) {
  return (
    <h2
      className={cn("text-[15px] font-semibold leading-snug tracking-tight", className)}
      {...props}
    />
  );
}

export function DialogDescription({ className, ...props }: ComponentProps<"p">) {
  return (
    <p className={cn("text-[12.5px] leading-relaxed text-muted-foreground", className)} {...props} />
  );
}

export function DialogFooter({ className, ...props }: ComponentProps<"div">) {
  return (
    <div
      className={cn("mt-5 flex items-center justify-end gap-2", className)}
      {...props}
    />
  );
}

/** Corner close affordance for optional dialogs. */
export function DialogX({ label }: { label: string }) {
  return (
    <DialogPrimitive.Close
      aria-label={label}
      className="absolute right-3 top-3 rounded-md p-1.5 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring/70"
    >
      <X className="size-4 stroke-[1.5]" />
    </DialogPrimitive.Close>
  );
}
