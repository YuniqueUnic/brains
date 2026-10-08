import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import i18n, { LANGUAGES } from "../../i18n";
import { Toggle } from "@base-ui-components/react/toggle";
import { ToggleGroup } from "@base-ui-components/react/toggle-group";
import { Slider } from "@base-ui-components/react/slider";
import { useTheme, type Theme } from "../../theme/ThemeProvider";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "../ui/dialog";
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
  document.documentElement.style.setProperty("--ui-font-size", `${settings.fontSize}px`);
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
            <ToggleGroup
              value={[settings.font]}
              onValueChange={(groupValue) =>
                update({ font: (groupValue[groupValue.length - 1] ?? "system") as UiSettings["font"] })
              }
              className="flex items-center gap-0.5 rounded-md border p-0.5"
            >
              {(
                [
                  ["system", t("fontSystem")],
                  ["serif", t("fontSerif")],
                  ["mono", t("fontMono")],
                ] as const
              ).map(([value, label]) => (
                <Toggle
                  key={value}
                  value={value}
                  aria-label={label}
                  className="cursor-pointer rounded px-2 py-1 text-[11.5px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground data-[pressed]:bg-selected data-[pressed]:text-foreground"
                >
                  {label}
                </Toggle>
              ))}
            </ToggleGroup>
          </SettingRow>

          <SettingRow label={t("settingsFontSize")}>
            <div className="flex items-center gap-2">
              <Slider.Root
                value={settings.fontSize}
                onValueChange={(value: number | number[]) =>
                  update({ fontSize: typeof value === "number" ? value : (value[0] ?? settings.fontSize) })
                }
                min={12}
                max={15}
                step={0.5}
                aria-label={t("settingsFontSize")}
                className="flex h-4 w-28 items-center"
              >
                <Slider.Control className="flex h-4 w-full items-center rounded">
                  <Slider.Track className="h-1 w-full rounded-full bg-muted">
                    <Slider.Indicator className="rounded-full bg-primary/60" />
                  </Slider.Track>
                  <Slider.Thumb className="size-3 cursor-pointer rounded-full border border-border bg-card shadow-menu" />
                </Slider.Control>
              </Slider.Root>
              <span className="w-10 text-right font-mono text-[11px]">{settings.fontSize}px</span>
            </div>
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
            <ToggleGroup
              value={[currentLanguage]}
              onValueChange={(groupValue) => {
                const next = groupValue[groupValue.length - 1];
                if (next && next !== i18n.language) void i18n.changeLanguage(next);
              }}
              className="flex items-center gap-0.5 rounded-md border p-0.5"
            >
              {(
                [
                  ["zh-CN", "中文"],
                  ["en", "English"],
                ] as const
              ).map(([value, label]) => (
                <Toggle
                  key={value}
                  value={value}
                  aria-label={label}
                  className="cursor-pointer rounded px-2 py-1 text-[11.5px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground data-[pressed]:bg-selected data-[pressed]:text-foreground"
                >
                  {label}
                </Toggle>
              ))}
            </ToggleGroup>
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
