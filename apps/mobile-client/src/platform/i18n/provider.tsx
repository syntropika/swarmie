import AsyncStorage from "@react-native-async-storage/async-storage";
import { useLocales } from "expo-localization";
import { createContext, useContext, useEffect, useMemo, useSyncExternalStore } from "react";
import type { ReactNode } from "react";

import { detectLocale } from "~/src/platform/i18n/locale";
import type { Locale, LocalePreference } from "~/src/platform/i18n/locale";
import { createLocalePreference } from "~/src/platform/i18n/preference";
import { createTranslator } from "~/src/platform/i18n/translate";
import type { Translate } from "~/src/platform/i18n/translate";

type LocaleContextValue = {
  locale: Locale;
  preference: LocalePreference;
  setPreference: (value: LocalePreference) => void;
  persistenceAvailable: boolean;
  t: Translate;
  formatDate: (value: number) => string;
  formatNumber: (value: number) => string;
};
const LocaleContext = createContext<LocaleContextValue | null>(null);
const preferenceKey = "swarmie.locale";

export function LocaleProvider({ children }: { children: ReactNode }) {
  const deviceLocales = useLocales();
  const preference = useMemo(
    () =>
      createLocalePreference({
        read: async () => await AsyncStorage.getItem(preferenceKey),
        write: async (value) => {
          await AsyncStorage.setItem(preferenceKey, value);
        },
      }),
    [],
  );
  const snapshot = useSyncExternalStore(
    preference.subscribe,
    preference.getSnapshot,
    preference.getSnapshot,
  );
  useEffect(() => {
    preference.restore().catch(() => {
      console.warn("Language preference initialization failed");
    });
  }, [preference]);
  const locale =
    snapshot.preference === "automatic"
      ? detectLocale(deviceLocales.map((device) => device.languageTag))
      : snapshot.preference;
  const value = useMemo<LocaleContextValue>(() => {
    const dates = new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "short" });
    const numbers = new Intl.NumberFormat(locale);
    return {
      locale,
      ...snapshot,
      setPreference: preference.setPreference,
      t: createTranslator(locale),
      formatDate: (date) => dates.format(date),
      formatNumber: (number) => numbers.format(number),
    };
  }, [locale, snapshot, preference]);
  return <LocaleContext value={value}>{children}</LocaleContext>;
}

export function useLocale(): LocaleContextValue {
  const value = useContext(LocaleContext);
  if (!value) {
    throw new Error("LocaleProvider is required");
  }
  return value;
}
