import { ArrowLeft, ArrowRight } from "lucide-react";
import { FilePlus, Folder, LogOut, PanelLeft, Search, Settings2, Upload, UserRound } from "lucide-react";
import { Menu } from "@base-ui-components/react/menu";
import { useTranslation } from "react-i18next";
import type { SessionInfo } from "../../api/client";
import { Button } from "../ui/button";
import { HoverMenu } from "../ui/hover-menu";
import { menuItemClass, MenuShortcut } from "../ui/menu";
import { Tooltip } from "../ui/tooltip";
import { BrandMark } from "../brand";

function GitHubMark() {
  return (
    <svg viewBox="0 0 16 16" aria-hidden className="size-4 shrink-0 fill-current">
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27s1.36.09 2 .27c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8Z" />
    </svg>
  );
}

export type UploadIntent = "files" | "folder" | "clipboard";

/**
 * Top bar (DESIGN.md §4-8): 52px, quiet chrome. 新建 is a direct button (it
 * has exactly one meaning); Upload is a hover-open / click-to-pin menu;
 * the account chip sits at the far right with logout one hover away. Search
 * lives next to the wordmark as the ⌘K palette trigger.
 */
const REPO_URL = "https://github.com/unic/brains";

export function TopBar({
  session,
  navigation,
  sidebarCollapsed,
  onToggleSidebar,
  onOpenPalette,
  onNewDocument,
  onUpload,
  onOpenSettings,
  onLogout,
}: {
  session: SessionInfo;
  navigation: { canBack: boolean; canForward: boolean; back: () => void; forward: () => void };
  sidebarCollapsed: boolean;
  onToggleSidebar: () => void;
  onOpenPalette: () => void;
  onNewDocument: () => void;
  onUpload: (intent: UploadIntent) => void;
  onOpenSettings: () => void;
  onLogout: () => void;
}) {
  const { t } = useTranslation("workspace");
  const { t: tCommon } = useTranslation("common");
  const permission = session.scopes.includes("admin:/")
    ? t("accessAdmin")
    : session.scopes.some((scope) => /^(write|admin):/.test(scope))
      ? t("accessWrite")
      : t("accessRead");
  const accountLabel = session.actor.startsWith("token:") ? permission : session.actor;

  return (
    <header className="flex h-[52px] shrink-0 items-center gap-1.5 border-b bg-background px-2 sm:gap-2 sm:px-3">
      <Tooltip content={sidebarCollapsed ? t("expandSidebar") : t("collapseSidebar")}>
        <Button
          size="icon"
          aria-label={sidebarCollapsed ? t("expandSidebar") : t("collapseSidebar")}
          aria-pressed={sidebarCollapsed}
          onClick={onToggleSidebar}
        >
          <PanelLeft />
        </Button>
      </Tooltip>

      <div className="flex items-center" role="group" aria-label={t("fileNavigation")}>
        <Tooltip content={t("fileBack")}>
          <Button size="icon" aria-label={t("fileBack")} data-testid="file-back" disabled={!navigation.canBack} onClick={navigation.back}><ArrowLeft /></Button>
        </Tooltip>
        <Tooltip content={t("fileForward")}>
          <Button size="icon" aria-label={t("fileForward")} data-testid="file-forward" disabled={!navigation.canForward} onClick={navigation.forward}><ArrowRight /></Button>
        </Tooltip>
      </div>

      <span className="hidden items-center gap-1.5 pr-1 text-sm font-semibold tracking-tight min-[440px]:flex">
        <BrandMark className="size-4 text-foreground" />
        Brains
      </span>

      {/* ⌘K trigger, styled as the palette it opens; compact icon form on
          narrow screens where the full bar cannot fit. */}
      <button
        data-testid="palette-trigger"
        onClick={onOpenPalette}
        aria-label={t("paletteOpen")}
        className="group hidden h-8 w-[220px] cursor-pointer items-center gap-2 rounded-[7px] border border-input bg-muted/60 px-2.5 text-body text-muted-foreground transition-colors hover:border-input hover:bg-muted focus-visible:outline-2 focus-visible:outline-ring/70 md:flex lg:w-[300px]"
      >
        <Search className="size-3.5 stroke-[1.5]" />
        <span className="flex-1 truncate text-left">{t("palettePlaceholder")}</span>
        <kbd>⌘K</kbd>
      </button>
      <Tooltip content={t("paletteOpen")}>
        <Button
          size="icon"
          aria-label={t("paletteOpen")}
          data-testid="palette-trigger-compact"
          onClick={onOpenPalette}
          className="md:hidden"
        >
          <Search />
        </Button>
      </Tooltip>

      <div className="ml-auto flex items-center gap-1.5">
        <Button
          variant="default"
          data-testid="new-file"
          aria-label={tCommon("newFile")}
          onClick={onNewDocument}
        >
          <FilePlus />
          <span className="hidden sm:inline">{tCommon("newFile")}</span>
          <span className="hidden sm:inline"><MenuShortcut>⌘N</MenuShortcut></span>
        </Button>

        <HoverMenu
          trigger={
            <Button variant="outline" aria-label={tCommon("upload")}>
              <Upload />
              <span className="hidden md:inline">{tCommon("upload")}</span>
            </Button>
          }
        >
          <Menu.Item
            className={menuItemClass()}
            onClick={() => onUpload("files")}
            data-testid="upload-files"
          >
            <Upload className="size-4 stroke-[1.5] text-muted-foreground" />
            {t("uploadFiles")}
          </Menu.Item>
          <Menu.Item
            className={menuItemClass()}
            onClick={() => onUpload("folder")}
            data-testid="upload-folder"
          >
            <Folder className="size-4 stroke-[1.5] text-muted-foreground" />
            {t("uploadFolder")}
          </Menu.Item>
          <Menu.Item
            className={menuItemClass()}
            onClick={() => onUpload("clipboard")}
            data-testid="upload-clipboard"
          >
            {t("uploadClipboard")}
          </Menu.Item>
        </HoverMenu>

        <Tooltip content={t("openSettings")}>
          <Button
            size="icon"
            aria-label={t("openSettings")}
            data-testid="open-settings"
            onClick={onOpenSettings}
          >
            <Settings2 />
          </Button>
        </Tooltip>
        <Tooltip content="GitHub">
          <a
            href={REPO_URL}
            target="_blank"
            rel="noreferrer"
            aria-label="GitHub"
            data-testid="github-link"
            className="hidden size-8 items-center justify-center rounded-[7px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring/70 md:inline-flex"
          >
            <GitHubMark />
          </a>
        </Tooltip>

        {/* Account chip, far right: hover reveals logout. */}
        <HoverMenu
          align="end"
          trigger={
            <button
              data-testid="account-button"
              aria-label={t("accountMenu")}
              className="flex h-8 min-w-0 cursor-pointer items-center gap-1.5 rounded-[7px] pl-1 pr-2 text-body-sm text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring/70"
            >
              <span className="flex size-6 shrink-0 items-center justify-center rounded-full bg-primary/15 text-meta font-semibold text-primary">
                <UserRound className="size-3.5 stroke-[1.5]" aria-hidden />
              </span>
              <span className="hidden min-w-0 max-w-[140px] truncate sm:inline">{accountLabel}</span>
            </button>
          }
        >
          <div className="flex items-center gap-2 px-2.5 py-1.5 text-xs text-muted-foreground">
            <span className="min-w-0 flex-1 truncate">{t("accessPermission", { permission })}</span>
          </div>
          <Menu.Item
            className={menuItemClass({ danger: true })}
            onClick={onLogout}
            data-testid="logout"
          >
            <LogOut className="size-4 stroke-[1.5]" />
            {tCommon("logout")}
          </Menu.Item>
        </HoverMenu>
      </div>
    </header>
  );
}
