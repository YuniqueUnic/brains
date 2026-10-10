import type { ReactNode } from "react";
import { CheckCheck, ListChecks, Square, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "./button";

/** Selection stays with the caller; only the common controls live here. */
export function BulkSelectionBar({ paths, selected, onSelectionChange, actions, disabled = false }: {
  paths: string[];
  selected: Set<string>;
  onSelectionChange: (next: Set<string>) => void;
  actions: ReactNode;
  disabled?: boolean;
}) {
  const { t } = useTranslation("workspace");
  return (
    <div role="toolbar" aria-label={t("select")} data-testid="bulk-selection-bar"
      className="mx-2 mb-1 flex shrink-0 flex-wrap items-center gap-0.5 rounded-md bg-muted/60 px-1.5 py-1 text-meta text-muted-foreground">
      <span aria-live="polite" className="mr-auto min-w-0 truncate">{t("selectedCount", { count: selected.size })}</span>
      <Button size="iconSm" aria-label={t("selectAll")} title={t("selectAll")} disabled={disabled || paths.length === 0}
        onClick={() => onSelectionChange(new Set(paths))}><CheckCheck /></Button>
      <Button size="iconSm" aria-label={t("invertSelection")} title={t("invertSelection")} disabled={disabled || paths.length === 0}
        onClick={() => onSelectionChange(new Set(paths.filter((path) => !selected.has(path))))}><ListChecks /></Button>
      <Button size="iconSm" aria-label={t("clearSelection")} title={t("clearSelection")} disabled={disabled || selected.size === 0}
        onClick={() => onSelectionChange(new Set())}><Square /></Button>
      {actions}
    </div>
  );
}

export function BulkSelectionToggle({ selecting, onToggle, disabled = false }: { selecting: boolean; onToggle: () => void; disabled?: boolean }) {
  const { t } = useTranslation("workspace");
  return <Button variant={selecting ? "secondary" : "ghost"} size="iconSm" aria-pressed={selecting}
    aria-label={selecting ? t("exitSelect") : t("select")} data-testid="toggle-select" disabled={disabled} onClick={onToggle}>
    {selecting ? <X /> : <ListChecks />}
  </Button>;
}
