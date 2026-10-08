import i18next from "i18next";
import { initReactI18next } from "react-i18next";

import en from "./locales/en";
import zhCN from "./locales/zh-CN";

export const NAMESPACES = ["common", "login", "workspace", "editor", "info"] as const;
export type Namespace = (typeof NAMESPACES)[number];

export const LANGUAGES = ["zh-CN", "en"] as const;
export type Language = (typeof LANGUAGES)[number];

const LANG_KEY = "brains-lang";

function initialLanguage(): Language {
  try {
    const stored = localStorage.getItem(LANG_KEY);
    if (stored && (LANGUAGES as readonly string[]).includes(stored)) {
      return stored as Language;
    }
  } catch {
    // localStorage unavailable: fall through to the default.
  }
  return "zh-CN";
}

function resources() {
  return {
    "zh-CN": zhCN,
    en,
  } as Record<Language, Record<Namespace, Record<string, unknown>>>;
}

const i18n = i18next.createInstance();

void i18n.use(initReactI18next).init({
  resources: resources(),
  lng: initialLanguage(),
  fallbackLng: "zh-CN",
  ns: NAMESPACES as unknown as string[],
  defaultNS: "common",
  interpolation: { escapeValue: false },
  returnNull: false,
});

// Keep <html lang> in sync for a11y and font selection.
document.documentElement.lang = i18n.language;
i18n.on("languageChanged", (lng) => {
  document.documentElement.lang = lng;
  try {
    localStorage.setItem(LANG_KEY, lng);
  } catch {
    // Persisting is best-effort.
  }
});

export function cycleLanguage(): void {
  const next = i18n.language === "zh-CN" ? "en" : "zh-CN";
  void i18n.changeLanguage(next);
}

export default i18n;
