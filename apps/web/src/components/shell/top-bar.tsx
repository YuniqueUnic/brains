import {
  Brain,
  FilePlus2,
  Folder,
  Languages,
  LogOut,
  MoreHorizontal,
  PanelLeft,
  Search,
  Upload,
  UserRound,
} from "lucide-react";
import { Menu } from "@base-ui-components/react/menu";
import { useTranslation } from "react-i18next";
import type { SessionInfo } from "../../api/client";
import { cycleLanguage } from "../../i18n";
import { Button } from "../ui/button";
import { menuPopupClass, menuItemClass, MenuShortcut } from "../ui/menu";
import { Tooltip } from "../ui/tooltip";

export type UploadIntent = "files" | "folder" | "clipboard";

/**
 * Top bar (DESIGN.md §4-8): 52px, three first-level actions only —
 * New (primary menu), Upload (secondary menu), ··· (utility). Search lives
 * next to the wordmark as the ⌘K palette trigger. Trash is sidebar
 * navigation, not a header state.
 */
export function TopBar({
  session,
  sidebarCollapsed,
  onToggleSidebar,
  onOpenPalette,
  onNewDocument,
  onUpload,
  onLogout,
}: {
  session: SessionInfo;
  sidebarCollapsed: boolean;
  onToggleSidebar: () => void;
  onOpenPalette: () => void;
  onNewDocument: () => void;
  onUpload: (intent: UploadIntent) => void;
  onLogout: () => void;
}) {
  const { t } = useTranslation("workspace");
  const { t: tCommon } = useTranslation("common");

  return (
    <header className="flex h-[52px] shrink-0 items-center gap-2 border-b bg-background px-3">
      <Tooltip content={sidebarCollapsed ? t("expandSidebar") : t("collapseSidebar")}>
        <Button size="icon" aria-label={t("collapseSidebar")} onClick={onToggleSidebar}>
          <PanelLeft />
        </Button>
      </Tooltip>

      <span className="flex items-center gap-1.5 pr-1 text-[14px] font-semibold tracking-tight">
        <Brain className="size-4 text-primary" />
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
                <Menu.Item
                  className={menuItemClass()}
                  onClick={() => cycleLanguage()}
                  data-testid="toggle-language"
                >
                  <Languages className="size-4 stroke-[1.5] text-muted-foreground" />
                  {tCommon("languageLabel")}
                  <MenuShortcut>···</MenuShortcut>
                </Menu.Item>
                <Menu.Separator className="mx-2 my-1 h-px bg-border" />
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
