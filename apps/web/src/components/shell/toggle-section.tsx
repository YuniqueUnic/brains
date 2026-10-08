import { Toggle } from "@base-ui-components/react/toggle";
import { ToggleGroup } from "@base-ui-components/react/toggle-group";

/** Small segmented control on Base UI ToggleGroup, shared by settings rows. */
export function ToggleGroupSection({
  value,
  onChange,
  options,
  className = "flex items-center gap-0.5 rounded-md border p-0.5",
}: {
  value: string;
  onChange: (next: string) => void;
  options: { value: string; label: string }[];
  className?: string;
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
          className="cursor-pointer rounded px-2 py-1 text-[11.5px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground data-[pressed]:bg-selected data-[pressed]:text-foreground"
        >
          {option.label}
        </Toggle>
      ))}
    </ToggleGroup>
  );
}
