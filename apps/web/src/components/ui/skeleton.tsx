import { FileText } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "../../lib/utils";

/**
 * Skeleton (loading placeholder): a muted shimmering block. Pure presentation
 * — render it while the real content is in flight so first paint never
 * jumps into place.
 */
export function Skeleton({
  className,
  style,
}: {
  className?: string;
  style?: React.CSSProperties;
}) {
  return <div aria-hidden className={cn("skeleton rounded", className)} style={style} />;
}

/**
 * Sidebar list placeholder (file tree / trash list during the first walk):
 * icon + label rows matching the real row rhythm.
 */
export function SidebarListSkeleton({ rows = 6 }: { rows?: number }) {
  const { t } = useTranslation("common");
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-0.5 overflow-hidden px-3 pb-1" data-testid="list-skeleton" role="status">
      <span className="sr-only">{t("loading")}</span>
      {Array.from({ length: rows }, (_, i) => (
        <div key={i} className="flex h-7 items-center gap-2">
          <FileText className="size-4 shrink-0 stroke-[1.5] text-faint" />
          <Skeleton className="h-3" style={{ width: `${34 + ((i * 27) % 40)}%` }} />
        </div>
      ))}
    </div>
  );
}
