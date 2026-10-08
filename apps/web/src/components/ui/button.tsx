import { cva, type VariantProps } from "class-variance-authority";
import type { ButtonHTMLAttributes } from "react";
import { cn } from "../../lib/utils";

/*
 * Button (DESIGN.md §8, §54): 32px control, 7px radius, 16px Lucide icons at
 * 1.5px stroke everywhere. The default is quiet; `default` (near-black) is the
 * single primary used by "+ New"; danger variants only for destructive acts.
 */
const buttonVariants = cva(
  "inline-flex cursor-pointer items-center justify-center gap-1.5 whitespace-nowrap rounded-[7px] text-[13px] font-medium transition-colors duration-150 focus-visible:outline-2 focus-visible:outline-ring/70 focus-visible:outline-offset-2 disabled:pointer-events-none disabled:opacity-45 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg]:size-4 [&_svg]:stroke-[1.5]",
  {
    variants: {
      variant: {
        default: "bg-foreground text-background hover:bg-foreground/88",
        primary: "bg-primary text-primary-foreground hover:bg-primary/90",
        outline: "border border-border bg-card text-foreground hover:bg-muted",
        secondary: "bg-secondary text-secondary-foreground hover:bg-secondary/75",
        ghost: "text-muted-foreground hover:bg-muted hover:text-foreground",
        destructive: "bg-destructive text-destructive-foreground hover:bg-destructive/90",
        dangerGhost: "text-destructive hover:bg-destructive/10 hover:text-destructive",
      },
      size: {
        sm: "h-8 px-3",
        icon: "h-8 w-8",
        iconSm: "h-7 w-7 [&_svg]:size-3.5",
      },
    },
    defaultVariants: { variant: "ghost", size: "sm" },
  },
);

export type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> &
  VariantProps<typeof buttonVariants>;

export function Button({ className, variant, size, ...props }: ButtonProps) {
  return (
    <button className={cn(buttonVariants({ variant, size }), className)} {...props} />
  );
}

export { buttonVariants };
