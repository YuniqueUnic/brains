import {
  FilePlus2,
  Folder,
  LogOut,
  MoreHorizontal,
  PanelLeft,
  Search,
  Settings2,
  Upload,
  UserRound,
} from "lucide-react";
import { Menu } from "@base-ui-components/react/menu";
import { useTranslation } from "react-i18next";
import type { SessionInfo } from "../../api/client";
import { Button } from "../ui/button";
import { menuPopupClass, menuItemClass } from "../ui/menu";
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
 * Top bar (DESIGN.md §4-8): 52px, three first-level actions only —
 * New (primary menu), Upload (secondary menu), ··· (utility). Search lives
 * next to the wordmark as the ⌘K palette trigger. Trash is sidebar
 * navigation, not a header state.
 */
const REPO_URL = "https://github.com/unic/brains";

export function TopBar({
  session,
  sidebarCollapsed,
  onToggleSidebar,
  onOpenPalette,
  onNewDocument,
  onUpload,
  onOpenSettings,
  onLogout,
}: {
  session: SessionInfo;
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

  return (
    <header className="flex h-[52px] shrink-0 items-center gap-2 border-b bg-background px-3">
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

      <span className="flex items-center gap-1.5 pr-1 text-[14px] font-semibold tracking-tight">
        <BrandMark className="size-4 text-foreground" />
        Brains
      </span>

      {/* ⌘K trigger, styled as the palette it opens. */}
      <button
        data-testid="palette-trigger"
        onClick={onOpenPalette}
        aria-label={t("paletteOpen")}
        className="group flex h-8 w-[300px] cursor-pointer items-center gap-2 rounded-[7px] border border-input bg-muted/60 px-2.5 text-[13px] text-muted-foreground transition-colors hover:border-input hover:bg-muted focus-visible:outline-2 focus-visible:outline-ring/70"
      >
        <Search className="size-3.5 stroke-[1.5]" />
        <span className="flex-1 truncate text-left">{t("palettePlaceholder")}</span>
        <kbd>⌘K</kbd>
      </button>

      <div className="ml-auto flex items-center gap-1.5">
        <Menu.Root>
          <Menu.Trigger
            render={
              <Button variant="default" data-testid="new-file" aria-label={tCommon("newFile")}>
                <FilePlus2 />
                {tCommon("newFile")}
              </Button>
            }
          />
          <Menu.Portal>
            <Menu.Positioner sideOffset={6} align="start">
              <Menu.Popup className={menuPopupClass}>
                <Menu.Item
                  className={menuItemClass()}
                  onClick={onNewDocument}
                  data-testid="new-document"
                >
                  <FilePlus2 className="size-4 stroke-[1.5] text-muted-foreground" />
                  {t("newDocument")}
                </Menu.Item>
              </Menu.Popup>
            </Menu.Positioner>
          </Menu.Portal>
        </Menu.Root>

        <Menu.Root>
          <Menu.Trigger
            render={
              <Button variant="outline" aria-label={tCommon("upload")}>
                <Upload />
                {tCommon("upload")}
              </Button>
            }
          />
          <Menu.Portal>
            <Menu.Positioner sideOffset={6} align="start">
              <Menu.Popup className={menuPopupClass}>
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
              </Menu.Popup>
            </Menu.Positioner>
          </Menu.Portal>
        </Menu.Root>

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
            className="inline-flex size-8 items-center justify-center rounded-[7px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring/70"
          >
            <GitHubMark />
          </a>
        </Tooltip>

        <Menu.Root>
          <Menu.Trigger
            render={
              <Button size="icon" aria-label={t("utilityMenu")} data-testid="utility-menu">
                <MoreHorizontal />
              </Button>
            }
          />
          <Menu.Portal>
            <Menu.Positioner sideOffset={6} align="end">
              <Menu.Popup className={menuPopupClass}>
                <div className="flex items-center gap-2 px-2.5 py-1.5 text-xs text-muted-foreground">
                  <UserRound className="size-3.5 stroke-[1.5]" />
                  <span className="min-w-0 flex-1 truncate">{session.actor}</span>
                </div>
                <Menu.Item
                  className={menuItemClass({ danger: true })}
                  onClick={onLogout}
                  data-testid="logout"
                >
                  <LogOut className="size-4 stroke-[1.5]" />
                  {tCommon("logout")}
                </Menu.Item>
              </Menu.Popup>
            </Menu.Positioner>
          </Menu.Portal>
        </Menu.Root>
      </div>
    </header>
  );
}
