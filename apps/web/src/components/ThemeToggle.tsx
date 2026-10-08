import { Monitor, Moon, Sun } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useTheme, type Theme } from "../theme/ThemeProvider";
import { Tooltip } from "./ui/tooltip";
import { Button } from "./ui/button";

const ICONS: Record<Theme, typeof Sun> = { light: Sun, dark: Moon, system: Monitor };

/** Cycle button: light → dark → system. The icon shows the current mode. */
export function ThemeToggle() {
  const { theme, cycle } = useTheme();
  const { t } = useTranslation("common");
  const Icon = ICONS[theme];
  const labels: Record<Theme, string> = {
    light: t("themeLight"),
    dark: t("themeDark"),
    system: t("themeSystem"),
  };
  return (
    <Tooltip content={`${labels[theme]} · ${t("themeCycle")}`}>
      <Button variant="ghost" size="icon" onClick={cycle} aria-label={labels[theme]}>
        <Icon />
      </Button>
    </Tooltip>
  );
}
