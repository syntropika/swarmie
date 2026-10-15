import { createContext, useContext, useMemo, useSyncExternalStore } from "react";
import type { ReactNode } from "react";

import { formatLocalizedDate } from "~/src/platform/i18n/date";
import type { Locale } from "~/src/platform/i18n/locale";
import {
  getInitialLocaleSnapshot,
  getLocaleSnapshot,
  setLocalePreference,
  subscribeLocale,
} from "~/src/platform/i18n/preference";
import type { LocalePreference } from "~/src/platform/i18n/preference";
import { createTranslator } from "~/src/platform/i18n/translate";
import type { Translate } from "~/src/platform/i18n/translate";

type LocaleContextValue = {
  locale: Locale;
  preference: LocalePreference;
  setPreference: (preference: LocalePreference) => void;
  t: Translate;
  formatDate: (value: string) => string;
};

const fallback: LocaleContextValue = {
  locale: "en",
  preference: "automatic",
  setPreference: setLocalePreference,
  t: createTranslator("en"),
  formatDate: (value) => formatLocalizedDate("en", value),
};
const LocaleContext = createContext<LocaleContextValue>(fallback);

/** Start with the static shell's language, then apply browser preferences after hydration. */
export function LocaleProvider({ children }: { children: ReactNode }) {
  const { locale, preference } = useSyncExternalStore(
    subscribeLocale,
    getLocaleSnapshot,
    getInitialLocaleSnapshot,
  );

  const value = useMemo<LocaleContextValue>(
    () => ({
      locale,
      preference,
      setPreference: setLocalePreference,
      t: createTranslator(locale),
      formatDate: (date) => formatLocalizedDate(locale, date),
    }),
    [locale, preference],
  );

  return <LocaleContext value={value}>{children}</LocaleContext>;
}

export function useLocale(): LocaleContextValue {
  return useContext(LocaleContext);
}
