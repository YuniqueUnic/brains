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
  name,
  files,
  editor,
  info,
}: {
  tab: MobileTab;
  onTabChange: (tab: MobileTab) => void;
  labels: Record<MobileTab, string>;
  /** Accessible name of the tab group itself (not one tab's label). */
  name: string;
  files: ReactNode;
  editor: ReactNode;
  info: ReactNode;
}) {
  const panes: Record<MobileTab, ReactNode> = { files, editor, info };
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="min-h-0 flex-1">
        {TABS.map(({ value }) => (
          <div
            key={value}
            id={`mobile-pane-${value}`}
            role="tabpanel"
            aria-labelledby={`mobile-tab-${value}`}
            className={cn("h-full min-h-0", tab !== value && "hidden")}
            data-testid={`mobile-pane-${value}`}
          >
            {panes[value]}
          </div>
        ))}
      </div>
      <div
        role="tablist"
        aria-label={name}
        className="grid h-12 shrink-0 grid-cols-3 border-t bg-card"
        data-testid="mobile-bottom-tabs"
        onKeyDown={(event) => {
          // Roving arrows: ← → cycle the tabs like a native tab bar.
          const index = TABS.findIndex(({ value }) => value === tab);
          const delta =
            event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
          if (delta === 0) return;
          event.preventDefault();
          const next = TABS[(index + delta + TABS.length) % TABS.length];
          onTabChange(next.value);
          document.getElementById(`mobile-tab-${next.value}`)?.focus();
        }}
      >
        {TABS.map(({ value, icon: Icon }) => (
          <button
            key={value}
            id={`mobile-tab-${value}`}
            role="tab"
            aria-selected={tab === value}
            aria-controls={`mobile-pane-${value}`}
            tabIndex={tab === value ? 0 : -1}
            data-state={tab === value ? "active" : "inactive"}
            onClick={() => onTabChange(value)}
            className="flex h-full cursor-pointer flex-col items-center justify-center gap-0.5 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring/70 data-[state=active]:bg-selected data-[state=active]:text-foreground"
          >
            <Icon className="size-4 stroke-[1.5]" />
            <span className="text-meta-sm leading-none">{labels[value]}</span>
          </button>
        ))}
      </div>
    </div>
  );
}
