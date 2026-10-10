import { Slider } from "@base-ui-components/react/slider";
import { RotateCcw } from "lucide-react";
import { cn } from "../../lib/utils";

/*
 * Shared slider row (Base UI Slider): track + fixed-width tabular value so
 * the layout never shifts while dragging, optional reset. The Control MUST
 * be `relative` — Base UI positions the thumb absolutely, and without a
 * positioned parent it anchors to the dialog/viewport and visibly drifts.
 */
export function SliderRow({
  label,
  ariaLabel,
  value,
  min,
  max,
  step,
  suffix = "",
  resetLabel,
  onChange,
  onReset,
  className,
}: {
  label?: string;
  /** Accessible name when the row has no visible label (footer). */
  ariaLabel?: string;
  value: number;
  min: number;
  max: number;
  step: number;
  suffix?: string;
  resetLabel?: string;
  onChange: (value: number) => void;
  onReset?: () => void;
  className?: string;
}) {
  return (
    <div className={cn("flex min-w-0 items-center gap-2", className)}>
      {label !== undefined && <span className="shrink-0">{label}</span>}
      <Slider.Root
        value={value}
        onValueChange={(next: number | number[]) => {
          const picked = typeof next === "number" ? next : next[0];
          if (typeof picked === "number") onChange(picked);
        }}
        min={min}
        max={max}
        step={step}
        aria-label={ariaLabel ?? (typeof label === "string" ? label : undefined)}
        className="flex h-4 min-w-0 flex-1 items-center"
      >
        <Slider.Control className="relative flex h-4 w-full touch-none select-none items-center">
          <Slider.Track className="h-1 w-full rounded-full bg-muted">
            <Slider.Indicator className="rounded-full bg-primary/60" />
          </Slider.Track>
          <Slider.Thumb className="size-3 cursor-pointer rounded-full border border-border bg-card shadow-menu" />
        </Slider.Control>
      </Slider.Root>
      <span className="w-11 shrink-0 text-right font-mono text-meta tabular-nums">
        {value}
        {suffix}
      </span>
      {onReset && (
        <button
          type="button"
          aria-label={resetLabel}
          title={resetLabel}
          className="cursor-pointer rounded p-0.5 text-muted-foreground hover:bg-muted hover:text-foreground"
          onClick={onReset}
        >
          <RotateCcw className="size-3.5 stroke-[1.5]" />
        </button>
      )}
    </div>
  );
}
