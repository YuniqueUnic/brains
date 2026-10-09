import { FileText, Files, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "../../lib/utils";

export type WorkspaceMode = "workspace" | "trash";

/**
 * Sidebar (DESIGN.UI.md §10): nav (All files / Trash), the mode's list
 * (workspace tree or trash items — the caller picks), and a one-line footer.
 * 248px default, 220–320 resize window (handled by the shell's resizable
 * panel).
 */
export function Sidebar({
  mode,
  onModeChange,
  fileCount,
  truncated,
  children,
}: {
  mode: WorkspaceMode;
  onModeChange: (mode: WorkspaceMode) => void;
  fileCount: number;
  truncated: boolean;
  children: React.ReactNode;
}) {
  const { t } = useTranslation("workspace");
  const navItem = (
    target: WorkspaceMode,
    label: string,
    Icon: typeof Files,
    testid?: string,
  ) => (
    <button
      type="button"
      data-testid={testid}
      aria-current={mode === target ? "true" : undefined}
      onClick={() => onModeChange(target)}
      className={cn(
        "flex h-7 cursor-pointer items-center gap-2 rounded-md px-2 text-body transition-colors",
        "focus-visible:outline-2 focus-visible:outline-ring/70",
        mode === target
          ? "bg-selected font-medium text-foreground"
          : "text-muted-foreground hover:bg-muted hover:text-foreground",
      )}
    >
      <Icon className="size-4 shrink-0 stroke-[1.5]" />
      {label}
    </button>
  );

  return (
    <div className="flex h-full min-h-0 flex-col bg-sidebar">
      <div className="flex h-11 shrink-0 items-center px-3">
        <span className="text-meta font-semibold tracking-wide text-faint uppercase">
          {t("sidebarWorkspace")}
        </span>
      </div>
      <nav className="flex shrink-0 flex-col gap-0.5 px-2 pb-1">
        {navItem("workspace", t("navAllFiles"), Files)}
        {navItem("trash", t("panelTrash"), Trash2, "toggle-trash")}
      </nav>
      <div className="mx-3 mb-1 h-px bg-border" />
      {children}
      <div className="flex h-8 shrink-0 items-center gap-2 px-3 text-meta text-faint">
        <FileText className="size-3.5 stroke-[1.5]" />
        <span>
          {mode === "workspace" ? t("footerFiles", { count: fileCount }) : t("footerItems", { count: fileCount })}
        </span>
        {truncated && <span data-testid="files-truncated">{t("truncated")}</span>}
      </div>
    </div>
  );
}
