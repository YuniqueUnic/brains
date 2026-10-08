import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import i18n, { LANGUAGES } from "../../i18n";
import { useTheme, type Theme } from "../../theme/ThemeProvider";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "../ui/dialog";
import { SliderRow } from "../ui/slider-row";
import { ToggleGroupSection } from "./toggle-section";

/*
 * UI-level settings (DESIGN feedback round 3): interface font, interface
 * font size, theme, language. Deliberately separate from the editor's own
 * font/scale preferences which live in the editor gear popover.
 */

export interface UiSettings {
  /** Interface font stack key; "system" is the curated default stack. */
  font: "system" | "serif" | "mono";
  /** Base UI font size in px (body text). */
  fontSize: number;
}

const UI_KEY = "brains-ui";

const FONT_STACKS: Record<UiSettings["font"], string> = {
  system: `"Inter", -apple-system, BlinkMacSystemFont, "PingFang SC", "Microsoft YaHei", sans-serif`,
  serif: `Georgia, "Iowan Old Style", "Songti SC", serif`,
  mono: `ui-monospace, SFMono-Regular, Menlo, Consolas, monospace`,
};

export function loadUiSettings(): UiSettings {
  try {
    const raw = localStorage.getItem(UI_KEY);
    if (raw) {
      return { font: "system", fontSize: 13, ...(JSON.parse(raw) as Partial<UiSettings>) };
    }
  } catch {
    /* ignore malformed state */
  }
  return { font: "system", fontSize: 13 };
}

function applyUiSettings(settings: UiSettings) {
  document.documentElement.style.setProperty("--font-sans", FONT_STACKS[settings.font]);
  // Interface size zooms the whole shell (px classes don't follow the body
  // font-size; zoom scales text, spacing and icons together). Portals live
  // inside body, so dialogs/toasts scale with everything else.
  document.body.style.setProperty("--ui-font-size", `${settings.fontSize}px`);
  document.body.style.zoom = String(settings.fontSize / 13);
}

export function UiSettingsProvider({ children }: { children: React.ReactNode }) {
  const [settings] = useState(loadUiSettings);
  useEffect(() => applyUiSettings(settings), [settings]);
  return <>{children}</>;
}

export function SettingsDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const { t } = useTranslation("workspace");
  const { t: tCommon } = useTranslation("common");
  const { theme, setTheme } = useTheme();
  const [settings, setSettings] = useState<UiSettings>(loadUiSettings);

  const update = (patch: Partial<UiSettings>) => {
    setSettings((prev) => {
      const next = { ...prev, ...patch };
      try {
        localStorage.setItem(UI_KEY, JSON.stringify(next));
      } catch {
        /* best-effort */
      }
      applyUiSettings(next);
      return next;
    });
  };

  const currentLanguage = (LANGUAGES as readonly string[]).includes(i18n.language)
    ? i18n.language
    : "zh-CN";

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent data-testid="settings-dialog" className="w-[min(24rem,94vw)]">
        <DialogHeader>
          <DialogTitle>{t("settingsTitle")}</DialogTitle>
          <DialogDescription>{t("settingsHint")}</DialogDescription>
        </DialogHeader>

        <div className="mt-4 flex flex-col gap-4">
          <SettingRow label={t("settingsFont")}>
            <ToggleGroupSection
              value={settings.font}
              onChange={(next) => update({ font: next as UiSettings["font"] })}
              options={[
                { value: "system", label: t("fontSystem") },
                { value: "serif", label: t("fontSerif") },
                { value: "mono", label: t("fontMono") },
              ]}
            />
          </SettingRow>

          <SettingRow label={t("settingsFontSize")}>
            <SliderRow
              value={settings.fontSize}
              min={8}
              max={32}
              step={1}
              suffix="px"
              onChange={(fontSize) => update({ fontSize })}
              className="w-44"
            />
          </SettingRow>

          <SettingRow label={t("settingsTheme")}>
            <ToggleGroupSection
              value={theme}
              onChange={(next) => setTheme(next as Theme)}
              options={[
                { value: "light", label: tCommon("themeLight") },
                { value: "dark", label: tCommon("themeDark") },
                { value: "system", label: tCommon("themeSystem") },
              ]}
            />
          </SettingRow>

          <SettingRow label={t("settingsLanguage")}>
            <ToggleGroupSection
              value={currentLanguage}
              onChange={(next) => {
                if (next !== i18n.language) void i18n.changeLanguage(next);
              }}
              options={[
                { value: "zh-CN", label: "中文" },
                { value: "en", label: "English" },
              ]}
            />
          </SettingRow>
        </div>
      </DialogContent>
    </Dialog>
  );
}

function SettingRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-3">
      <span className="shrink-0 text-[12.5px] text-foreground">{label}</span>
      {children}
    </div>
  );
}
