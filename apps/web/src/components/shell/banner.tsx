import { TriangleAlert, X } from "lucide-react";
import { useTranslation } from "react-i18next";

/** Floating status banner: auth loss, restore notices, binary rejections. */
export function Banner({ message, onClose }: { message: string; onClose: () => void }) {
  const { t } = useTranslation("common");
  return (
    <div className="pane-in pointer-events-none fixed inset-x-0 top-14 z-30 flex justify-center px-4">
      <div
        className="pointer-events-auto flex max-w-xl items-start gap-2 rounded-[8px] border bg-card px-3 py-2 text-xs shadow-overlay"
        data-testid="banner"
        role="status"
      >
        <TriangleAlert className="mt-0.5 size-3.5 shrink-0 stroke-[1.5] text-warning" />
        <span className="min-w-0 flex-1 leading-relaxed">{message}</span>
        <button
          onClick={onClose}
          aria-label={t("close")}
          className="hit cursor-pointer rounded p-0.5 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring/70"
        >
          <X className="size-3.5 stroke-[1.5]" />
        </button>
      </div>
    </div>
  );
}
