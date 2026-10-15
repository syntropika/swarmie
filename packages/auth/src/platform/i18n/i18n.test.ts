import { describe, expect, test } from "bun:test";

import { en } from "~/src/platform/i18n/catalogs/en";
import type { MessageKey } from "~/src/platform/i18n/catalogs/en";
import { formatLocalizedDate } from "~/src/platform/i18n/date";
import { detectLocale, isLocale, locales, matchLocale } from "~/src/platform/i18n/locale";
import { catalogs, createTranslator } from "~/src/platform/i18n/translate";

function placeholders(value: string): string[] {
  return [...value.matchAll(/\{(?<placeholder>[^}]+)\}/gu)]
    .map((match) => match.groups?.placeholder ?? "")
    .toSorted();
}

describe("browser language preferences", () => {
  test("chooses the first supported browser language, with English fallback", () => {
    expect(detectLocale(["nl-NL", "es-MX", "en-US"])).toBe("es");
    expect(detectLocale(["pt-BR"])).toBe("pt");
    expect(detectLocale(["ja-JP"])).toBe("ja");
    expect(detectLocale(["ar", "ko-KR"])).toBe("en");
    expect(detectLocale([])).toBe("en");
    expect(matchLocale("zh-Hans-SG")).toBe("zh-CN");
    expect(matchLocale("zh_SG")).toBe("zh-CN");
    expect(matchLocale("zh-Hant-TW")).toBeUndefined();
    expect(matchLocale("zh-TW")).toBeUndefined();
  });

  test("respects preference order and skips unsupported Chinese scripts", () => {
    expect(detectLocale(["fr-CA", "es", "en"])).toBe("fr");
    expect(detectLocale(["es", "fr-CA", "en"])).toBe("es");
    expect(detectLocale(["zh-Hant-TW", "de-AT"])).toBe("de");
    expect(detectLocale(["zh-Hant", "zh-Hans-CN"])).toBe("zh-CN");
    expect(detectLocale(["zh-Hant-HK"])).toBe("en");
    expect(matchLocale("EN_us")).toBe("en");
    expect(matchLocale("__proto__")).toBeUndefined();
    expect(isLocale("zh-CN")).toBe(true);
    expect(isLocale("__proto__")).toBe(false);
    expect(isLocale(null)).toBe(false);
  });
});

describe("translation contracts", () => {
  test("every catalog has all keys, nonempty messages, and identical interpolation names", () => {
    const keys = Object.keys(en).filter((key): key is MessageKey => Object.hasOwn(en, key));
    for (const locale of locales) {
      const catalog = catalogs[locale];
      expect(Object.keys(catalog).toSorted()).toEqual(keys.toSorted());
      for (const key of keys) {
        expect(catalog[key].trim().length).toBeGreaterThan(0);
        expect(placeholders(catalog[key])).toEqual(placeholders(en[key]));
      }
    }
  });

  test("interpolates names as text, without recursively treating user text as templates", () => {
    const t = createTranslator("es");
    expect(t("account.welcome", { name: "Ada" })).toBe("Hola, Ada.");
    expect(t("account.welcome", { name: "{date}<script>" })).toBe("Hola, {date}<script>.");
    expect(createTranslator("ja")("keys.removeNamed", { name: "Laptop" })).toContain("Laptop");
  });

  test("uses the selected locale for dates and an intentional invalid-date fallback", () => {
    const date = "2026-09-19T12:00:00Z";
    expect(formatLocalizedDate("es", date)).toBe(
      new Intl.DateTimeFormat("es", { dateStyle: "medium" }).format(new Date(date)),
    );
    expect(formatLocalizedDate("ja", date)).not.toBe(formatLocalizedDate("en", date));
    expect(formatLocalizedDate("fr", "invalid")).toBe(catalogs.fr["keys.dateUnavailable"]);
  });
});

/** These call sites are checked by tsc but never executed. */
function translationTypes() {
  const t = createTranslator("en");
  // @ts-expect-error Unknown keys must not enter the interface.
  t("unknown.key");
  // @ts-expect-error Required interpolation arguments cannot be omitted.
  t("account.welcome");
  // @ts-expect-error Wrong interpolation names must be rejected.
  t("account.welcome", { email: "test@example.com" });
  // @ts-expect-error Static copy accepts no interpolation arguments.
  t("common.cancel", { name: "Ada" });
}
void translationTypes;
