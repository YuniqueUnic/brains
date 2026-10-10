import type { ComponentProps } from "react";
import { cn } from "../../lib/utils";

export function Input({ className, ...props }: ComponentProps<"input">) {
  return (
    <input
      className={cn(
        "h-8 w-full rounded-[7px] border border-input bg-background px-3 text-body text-foreground",
        "placeholder:text-muted-foreground/70",
        "focus-visible:border-ring focus-visible:outline-2 focus-visible:outline-ring/35 focus-visible:-outline-offset-1",
        "disabled:cursor-not-allowed disabled:opacity-50",
        className,
      )}
      {...props}
    />
  );
}
