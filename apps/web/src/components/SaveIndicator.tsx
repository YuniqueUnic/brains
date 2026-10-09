import { cn } from "../lib/utils";

export type SaveTone = "saved" | "pending" | "error" | "neutral";

/**
 * Traffic-light save capsule (syzygy EditorSaveIndicator): a tone dot with a
 * halo plus the state text. The text doubles as the e2e contract, so keep the
 * exact labels from the state machine.
 */
export function SaveIndicator({
  tone,
  label,
  busy = false,
  title,
}: {
  tone: SaveTone;
  label: string;
  busy?: boolean;
  title?: string;
}) {
  const toneColor =
    tone === "saved"
      ? "var(--success)"
      : tone === "error"
        ? "var(--destructive)"
        : tone === "pending"
          ? "var(--warning)"
          : "var(--muted-foreground)";
  return (
    <span
      data-testid="editor-status"
      role={tone === "error" ? "alert" : "status"}
      aria-live="polite"
      title={title}
      className={cn(
        "inline-flex h-[22px] items-center gap-1.5 rounded-full px-2 text-meta font-medium",
        "select-none whitespace-nowrap",
      )}
      style={{
        backgroundColor: `color-mix(in srgb, ${toneColor} 10%, transparent)`,
        color: `color-mix(in srgb, ${toneColor} 80%, var(--foreground))`,
      }}
    >
      <span
        aria-hidden
        className={cn("inline-block size-[7px] rounded-full", busy && "save-pulse")}
        style={{
          backgroundColor: toneColor,
          boxShadow: `0 0 0 3px color-mix(in srgb, ${toneColor} 22%, transparent)`,
        }}
      />
      {label}
    </span>
  );
}
