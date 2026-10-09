import { Toggle } from "@base-ui-components/react/toggle";
import { ToggleGroup } from "@base-ui-components/react/toggle-group";
import type { ReactNode } from "react";

/**
 * Segmented control on Base UI ToggleGroup — the ONE segmented control for
 * the whole shell (settings rows, editor view mode, trash view mode).
 * Options carry an optional leading icon; dense fits inline popover rows.
 */
export function ToggleGroupSection({
  value,
  onChange,
  options,
  className = "flex items-center gap-0.5 rounded-md border p-0.5",
  dense = false,
  compactOnMobile = false,
}: {
  value: string;
  onChange: (next: string) => void;
  options: { value: string; label: string; icon?: ReactNode }[];
  className?: string;
  dense?: boolean;
  compactOnMobile?: boolean;
}) {
  return (
    <ToggleGroup
      value={[value]}
      onValueChange={(groupValue) => {
        const next = groupValue[groupValue.length - 1];
        if (next) onChange(next);
      }}
      className={className}
    >
      {options.map((option) => (
        <Toggle
          key={option.value}
          value={option.value}
          aria-label={option.label}
          className={
            dense
              ? "cursor-pointer rounded px-1.5 py-0.5 font-medium text-meta-sm text-muted-foreground transition-colors hover:bg-muted hover:text-foreground data-[pressed]:bg-selected data-[pressed]:text-foreground"
              : "flex h-6 cursor-pointer items-center gap-1 rounded-md px-2 font-medium text-meta-lg text-muted-foreground transition-colors hover:bg-muted hover:text-foreground data-[pressed]:bg-selected data-[pressed]:text-foreground"
          }
        >
          {option.icon}
          <span className={compactOnMobile && option.icon ? "hidden sm:inline" : undefined}>{option.label}</span>
        </Toggle>
      ))}
    </ToggleGroup>
  );
}
