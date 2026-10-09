import { AlertDialog as AlertDialogPrimitive } from "@base-ui-components/react/alert-dialog";
import type { ComponentProps, ReactNode } from "react";
import { cn } from "../../lib/utils";
import { buttonVariants } from "./button";

/*
 * Danger confirmation (DESIGN.md §17, §55-④): small Linear-style dialog,
 * destructive action button only when the act is truly irreversible.
 * Controlled usage: <AlertDialog open onOpenChange><AlertDialogContent …>.
 */
export const AlertDialog = AlertDialogPrimitive.Root;

export function AlertDialogContent({
  className,
  children,
  ...props
}: ComponentProps<typeof AlertDialogPrimitive.Popup> & { children: ReactNode }) {
  return (
    <AlertDialogPrimitive.Portal>
      <AlertDialogPrimitive.Backdrop className="fixed inset-0 z-40 bg-black/35 backdrop-blur-[2px] fade-in" />
      <AlertDialogPrimitive.Popup
        className={cn(
          "fixed left-1/2 top-1/2 z-50 flex w-[min(24rem,94vw)] -translate-x-1/2 -translate-y-1/2 flex-col gap-1.5",
          "rounded-[12px] border bg-card p-5 text-foreground shadow-overlay overlay-in [zoom:var(--ui-zoom,1)]",
          className,
        )}
        {...props}
      >
        {children}
      </AlertDialogPrimitive.Popup>
    </AlertDialogPrimitive.Portal>
  );
}

export function AlertDialogTitle({ className, ...props }: ComponentProps<"h2">) {
  return (
    <h2
      className={cn("text-[15px] font-semibold leading-snug tracking-tight", className)}
      {...props}
    />
  );
}

export function AlertDialogDescription({ className, ...props }: ComponentProps<"p">) {
  return (
    <p className={cn("text-[12.5px] leading-relaxed text-muted-foreground", className)} {...props} />
  );
}

export function AlertDialogFooter({ className, ...props }: ComponentProps<"div">) {
  return (
    <div className={cn("mt-4 flex items-center justify-end gap-2", className)} {...props} />
  );
}

export function AlertDialogCancel({
  className,
  ...props
}: ComponentProps<typeof AlertDialogPrimitive.Close>) {
  return (
    <AlertDialogPrimitive.Close
      autoFocus
      className={cn(buttonVariants({ variant: "outline", size: "sm" }), "cursor-pointer", className)}
      {...props}
    />
  );
}

export function AlertDialogAction({
  className,
  ...props
}: ComponentProps<"button">) {
  return (
    <button
      className={cn(
        buttonVariants({ variant: "destructive", size: "sm" }),
        "cursor-pointer",
        className,
      )}
      {...props}
    />
  );
}

/** Pre-styled danger/cancel pair matching DESIGN.md §17. */
export const alertActionClass = (extra: string) => extra;
export const alertCancelClass = () => "";
