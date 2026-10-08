import { cn } from "../../lib/utils";

/*
 * Shared visuals for Base UI menus — dropdown (Menu) and context menu
 * (ContextMenu) use the same parts and therefore the same classes.
 * Kept as class helpers so call sites compose `Menu.Item`/`ContextMenu.Item`
 * directly instead of through yet another layer.
 */

export const menuPopupClass = cn(
  "z-50 min-w-[184px] overflow-hidden rounded-[8px] border bg-card p-1 text-[13px] text-foreground shadow-menu overlay-in",
);

export function menuItemClass(options?: { danger?: boolean; className?: string }) {
  return cn(
    "flex cursor-pointer items-center gap-2 rounded-md px-2.5 py-[6px] outline-none transition-colors",
    "data-[highlighted]:bg-muted data-[highlighted]:text-foreground",
    "data-[disabled]:pointer-events-none data-[disabled]:opacity-45",
    options?.danger &&
      "text-destructive data-[highlighted]:bg-destructive/10 data-[highlighted]:text-destructive",
    options?.className,
  );
}

/** Right-aligned key hint inside a menu row. */
export function MenuShortcut({ children }: { children: React.ReactNode }) {
  return <kbd className="ml-auto shrink-0">{children}</kbd>;
}
