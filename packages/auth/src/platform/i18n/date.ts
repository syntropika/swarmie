import type { Locale } from "~/src/platform/i18n/locale";
import { createTranslator } from "~/src/platform/i18n/translate";

/** Dates use the selected UI locale and the browser's local time zone. */
export function formatLocalizedDate(locale: Locale, value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return createTranslator(locale)("keys.dateUnavailable");
  }
  return new Intl.DateTimeFormat(locale, { dateStyle: "medium" }).format(date);
}
