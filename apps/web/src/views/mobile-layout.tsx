import type { ReactNode } from "react";
import { FolderOpen, Info, SquarePen } from "lucide-react";
import { cn } from "../lib/utils";

/*
 * Mobile pane switch (the three-tab pattern from Elizabeth): a bottom tab
 * bar swaps files / editor / info. The editor pane STAYS MOUNTED — hidden
 * with CSS, never unmounted — so the CodeMirror view and its unsaved buffer
 * survive every tab switch.
 */

export type MobileTab = "files" | "editor" | "info";

const TABS: { value: MobileTab; icon: typeof FolderOpen }[] = [
  { value: "files", icon: FolderOpen },
  { value: "editor", icon: SquarePen },
  { value: "info", icon: Info },
];

export function MobileLayout({
  tab,
  onTabChange,
  labels,
  files,
  editor,
  info,
}: {
  tab: MobileTab;
  onTabChange: (tab: MobileTab) => void;
  labels: Record<MobileTab, string>;
  files: ReactNode;
  editor: ReactNode;
  info: ReactNode;
}) {
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="min-h-0 flex-1">
        <div className={cn("h-full min-h-0", tab !== "files" && "hidden")} data-testid="mobile-pane-files">
          {files}
        </div>
        <div className={cn("h-full min-h-0", tab !== "editor" && "hidden")} data-testid="mobile-pane-editor">
          {editor}
        </div>
        <div className={cn("h-full min-h-0", tab !== "info" && "hidden")} data-testid="mobile-pane-info">
          {info}
        </div>
      </div>
      <div
        role="tablist"
        aria-label={labels.files}
        className="grid h-12 shrink-0 grid-cols-3 border-t bg-card"
        data-testid="mobile-bottom-tabs"
      >
        {TABS.map(({ value, icon: Icon }) => (
          <button
            key={value}
            role="tab"
            aria-selected={tab === value}
            data-state={tab === value ? "active" : "inactive"}
            onClick={() => onTabChange(value)}
            className="flex h-full cursor-pointer flex-col items-center justify-center gap-0.5 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground data-[state=active]:bg-selected data-[state=active]:text-foreground"
          >
            <Icon className="size-4 stroke-[1.5]" />
            <span className="text-[10.5px] leading-none">{labels[value]}</span>
          </button>
        ))}
      </div>
    </div>
  );
}
