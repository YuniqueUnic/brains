import { Toggle } from "@base-ui-components/react/toggle";
import { ToggleGroup } from "@base-ui-components/react/toggle-group";
import { Switch } from "@base-ui-components/react/switch";
import { Popover } from "@base-ui-components/react/popover";
import { Settings2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "../ui/button";
import { SliderRow } from "../ui/slider-row";
import { type EditorSettings } from "./editor-settings";

/**
 * Editor preferences popover (line numbers, wrap, font family, scale).
 * The editor font is independent of the UI font.
 */
export function EditorSettingsPopover({
  settings,
  onChange,
}: {
  settings: EditorSettings;
  onChange: (patch: Partial<EditorSettings>) => void;
}) {
  const { t } = useTranslation("editor");
  return (
    <Popover.Root>
      <Popover.Trigger
        render={<Button size="icon" aria-label={t("settingsLabel")} data-testid="editor-settings" />}
      >
        <Settings2 />
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Positioner align="end" sideOffset={6}>
          <Popover.Popup className="z-50 w-[248px] rounded-[10px] border bg-card p-3 text-[12.5px] shadow-menu overlay-in [zoom:var(--ui-zoom,1)]">
            <SwitchRow
              label={t("lineNumbersLabel")}
              checked={settings.lineNumbers}
              onChange={(checked) => onChange({ lineNumbers: checked })}
            />
            <SwitchRow
              label={t("wrapLabel")}
              checked={settings.wrap}
              onChange={(checked) => onChange({ wrap: checked })}
            />
            <div className="flex items-center justify-between py-1.5">
              <span>{t("fontLabel")}</span>
              <ToggleGroup
                value={[settings.font]}
                onValueChange={(groupValue) =>
                  onChange({ font: (groupValue[groupValue.length - 1] ?? "sans") as EditorSettings["font"] })
                }
                className="flex items-center gap-0.5 rounded-md border p-0.5"
              >
                {(
                  [
                    ["sans", t("fontSans")],
                    ["serif", t("fontSerif")],
                    ["mono", t("fontMono")],
                  ] as const
                ).map(([value, label]) => (
                  <Toggle
                    key={value}
                    value={value}
                    aria-label={label}
                    className="cursor-pointer rounded px-1.5 py-0.5 text-[10.5px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground data-[pressed]:bg-selected data-[pressed]:text-foreground"
                  >
                    {label}
                  </Toggle>
                ))}
              </ToggleGroup>
            </div>
            <div className="py-1.5">
              <SliderRow
                label={t("scaleLabel")}
                value={settings.scale}
                min={80}
                max={160}
                step={5}
                suffix="%"
                resetLabel={t("scaleReset")}
                onChange={(scale) => onChange({ scale })}
                onReset={() => onChange({ scale: 100 })}
              />
            </div>
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  );
}

function SwitchRow({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <div className="flex items-center justify-between py-1.5">
      <span>{label}</span>
      <Switch.Root
        checked={checked}
        onCheckedChange={(next) => onChange(next === true)}
        className="relative h-5 w-8 cursor-pointer rounded-full bg-muted transition-colors data-[checked]:bg-primary"
      >
        <Switch.Thumb className="absolute left-0.5 top-0.5 size-4 rounded-full bg-card shadow transition-transform data-[checked]:translate-x-3" />
      </Switch.Root>
    </div>
  );
}
