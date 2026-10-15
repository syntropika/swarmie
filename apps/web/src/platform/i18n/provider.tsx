import { createContext, useContext, useMemo } from "react";
import type { ReactNode } from "react";

import type { Locale } from "~/src/platform/i18n/locale";
import { createTranslator } from "~/src/platform/i18n/translate";
import type { Translate } from "~/src/platform/i18n/translate";

type LocaleContextValue = {
  locale: Locale;
  t: Translate;
  formatTime: (hour: number, minute: number) => string;
};

function formatTime(locale: Locale, hour: number, minute: number): string {
  return new Intl.DateTimeFormat(locale, {
    hour: "numeric",
    minute: "2-digit",
    timeZone: "UTC",
  }).format(new Date(Date.UTC(2026, 0, 1, hour, minute)));
}

const LocaleContext = createContext<LocaleContextValue>({
  locale: "en",
  t: createTranslator("en"),
  formatTime: (hour, minute) => formatTime("en", hour, minute),
});

/** Both renders use the same path-derived locale; no post-hydration language replacement. */
export function LocaleProvider({ locale, children }: { locale: Locale; children: ReactNode }) {
  const value = useMemo<LocaleContextValue>(
    () => ({
      locale,
      t: createTranslator(locale),
      formatTime: (hour, minute) => formatTime(locale, hour, minute),
    }),
    [locale],
  );
  return <LocaleContext value={value}>{children}</LocaleContext>;
}

export function useLocale(): LocaleContextValue {
  return useContext(LocaleContext);
}
