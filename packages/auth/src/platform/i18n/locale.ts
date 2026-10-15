export const locales = ["en", "es", "fr", "de", "pt", "it", "ja", "zh-CN"] as const;
export type Locale = (typeof locales)[number];

export function isLocale(value: unknown): value is Locale {
  return typeof value === "string" && locales.some((locale) => locale === value);
}

/** Unsupported scripts are not silently presented as another Chinese writing system. */
export function matchLocale(language: string): Locale | undefined {
  const normalized = language.toLowerCase().replaceAll("_", "-");
  if (
    normalized === "zh" ||
    normalized === "zh-cn" ||
    normalized === "zh-sg" ||
    normalized.startsWith("zh-hans")
  ) {
    return "zh-CN";
  }
  const [base] = normalized.split("-");
  if (base !== "zh" && isLocale(base)) {
    return base;
  }
  return undefined;
}

export function detectLocale(languages: readonly string[]): Locale {
  for (const language of languages) {
    const locale = matchLocale(language);
    if (locale) {
      return locale;
    }
  }
  return "en";
}
