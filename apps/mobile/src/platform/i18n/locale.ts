import languageNames from "~/locales.json";

export { default as languageNames } from "~/locales.json";
export type Locale = keyof typeof languageNames;
export type LocalePreference = Locale | "automatic";
export const locales = Object.keys(languageNames).filter(isLocale);

export function isLocale(value: unknown): value is Locale {
  return typeof value === "string" && Object.hasOwn(languageNames, value);
}

/** Match regional variants without substituting simplified Chinese for an unsupported script. */
export function matchLocale(language: string): Locale | undefined {
  const tag = language.toLowerCase().replaceAll("_", "-");
  if (!/^[a-z]{2,3}(?:-[a-z0-9]{1,8})*$/u.test(tag)) {
    return undefined;
  }
  if (tag.split("-").includes("hant")) {
    return undefined;
  }
  if (tag === "zh" || /^zh-(?:cn|sg|hans)(?:-|$)/u.test(tag)) {
    return "zh-CN";
  }
  const [base] = tag.split("-");
  return base !== "zh" && isLocale(base) ? base : undefined;
}

export function detectLocale(languages: readonly string[]): Locale {
  for (const language of languages) {
    const match = matchLocale(language);
    if (match) {
      return match;
    }
  }
  return "en";
}
