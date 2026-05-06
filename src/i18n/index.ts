import { useState, useCallback, useMemo, createContext, useContext } from "react";
import zhCN from "./zh-CN";
import enUS from "./en-US";

export type Locale = "zh-CN" | "en-US";

const messages: Record<Locale, Record<string, string>> = {
  "zh-CN": zhCN,
  "en-US": enUS,
};

function detectLocale(): Locale {
  try {
    const lang = navigator.language;
    if (lang.startsWith("zh")) return "zh-CN";
  } catch {
    // ignore
  }
  return "en-US";
}

export interface I18nContextType {
  locale: Locale;
  setLocale: (locale: Locale) => void;
  t: (key: string, params?: Record<string, string | number>) => string;
}

export const I18nContext = createContext<I18nContextType>({
  locale: "zh-CN",
  setLocale: () => {},
  t: (key: string) => key,
});

export function useI18nManager() {
  const [locale, setLocale] = useState<Locale>(detectLocale);

  const t = useCallback(
    (key: string, params?: Record<string, string | number>): string => {
      let msg = messages[locale]?.[key] ?? messages["en-US"]?.[key] ?? key;
      if (params) {
        for (const [k, v] of Object.entries(params)) {
          msg = msg.replace(`{${k}}`, String(v));
        }
      }
      return msg;
    },
    [locale],
  );

  return useMemo(() => ({ locale, setLocale, t }), [locale, t]);
}

export function useTranslation() {
  return useContext(I18nContext);
}
