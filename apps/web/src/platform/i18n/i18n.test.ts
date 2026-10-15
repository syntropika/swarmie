import { describe, expect, test } from "bun:test";

import { en, isMessageKey } from "~/src/platform/i18n/catalogs/en";
import {
  alternateLocaleLinks,
  canonicalLocaleUrl,
  isLocale,
  localeFromPath,
  localePath,
  locales,
  preferredLocaleFromAcceptLanguage,
} from "~/src/platform/i18n/locale";
import { catalogs, createTranslator } from "~/src/platform/i18n/translate";

function placeholders(value: string): string[] {
  return [...value.matchAll(/\{(?<name>[^}]+)\}/gu)]
    .map((match) => match.groups?.name ?? "")
    .toSorted();
}

describe("public locale paths", () => {
  test("English uses the root while other languages use distinct prefixed paths", () => {
    expect(locales).toHaveLength(8);
    expect(new Set(locales).size).toBe(locales.length);
    for (const locale of locales) {
      expect(isLocale(locale)).toBe(true);
      const path = locale === "en" ? "/" : `/${locale}/`;
      expect(localePath(locale)).toBe(path);
      expect(localeFromPath(localePath(locale))).toBe(locale);
      expect(canonicalLocaleUrl(locale)).toBe(`https://swarmie.ai${path}`);
      if (locale !== "en") {
        expect(localeFromPath(`/${locale}`)).toBe(locale);
      }
    }
    expect(isLocale("zh-cn")).toBe(false);
  });

  test("only an exact language path selects a marketing locale", () => {
    for (const path of [
      "/en/",
      "/en",
      "/api/subscribe",
      "/es/api/subscribe",
      "/es/extra",
      "/xx/",
      "/ES/",
      "/es//",
    ]) {
      expect(localeFromPath(path)).toBeUndefined();
    }
    expect(localeFromPath("/es/")).toBe("es");
    expect(localeFromPath("/it/")).toBe("it");
    expect(localeFromPath("/zh-CN/")).toBe("zh-CN");
    expect(localeFromPath("/")).toBe("en");
  });

  test("alternate language links match canonical URLs and use English as the default", () => {
    const links = alternateLocaleLinks();
    expect(links).toHaveLength(locales.length + 1);
    expect(links.filter((link) => link.hrefLang === "x-default")).toEqual([
      { rel: "alternate", hrefLang: "x-default", href: canonicalLocaleUrl("en") },
    ]);
    for (const locale of locales) {
      expect(links.filter((link) => link.hrefLang === locale)).toEqual([
        { rel: "alternate", hrefLang: locale, href: canonicalLocaleUrl(locale) },
      ]);
    }
  });
});

describe("browser language preference at the root", () => {
  test("chooses the highest-quality supported language and respects ties", () => {
    expect(preferredLocaleFromAcceptLanguage("es-ES,es;q=0.9,en-US;q=0.8")).toBe("es");
    expect(preferredLocaleFromAcceptLanguage("it;q=0.7,es;q=0.9")).toBe("es");
    expect(preferredLocaleFromAcceptLanguage("fr-CA,de;q=1")).toBe("fr");
    expect(preferredLocaleFromAcceptLanguage("en-US,en;q=0.9,es;q=0.8")).toBe("en");
    expect(preferredLocaleFromAcceptLanguage("zh-Hans-CN,ja;q=0.7")).toBe("zh-CN");
    expect(preferredLocaleFromAcceptLanguage("zh-Hant-TW,ja;q=0.7")).toBe("ja");
  });

  test("ignores rejected or unsupported ranges and defaults to English", () => {
    expect(preferredLocaleFromAcceptLanguage("es;q=0,it;q=0.6")).toBe("it");
    expect(preferredLocaleFromAcceptLanguage("es;q=invalid,ko;q=0.9")).toBe("en");
    expect(preferredLocaleFromAcceptLanguage("ko-KR,*;q=0.8")).toBe("en");
    expect(preferredLocaleFromAcceptLanguage(undefined)).toBe("en");
  });
});

describe("public-site translation contract", () => {
  test("every locale provides all messages and preserves interpolation names", () => {
    const keys = Object.keys(en).filter(isMessageKey);
    for (const locale of locales) {
      expect(Object.keys(catalogs[locale]).toSorted()).toEqual([...keys].toSorted());
      for (const key of keys) {
        expect(catalogs[locale][key].trim().length).toBeGreaterThan(0);
        expect(placeholders(catalogs[locale][key])).toEqual(placeholders(en[key]));
      }
    }
  });

  test("safe newsletter errors translate independently from provider response payloads", () => {
    expect(createTranslator("es")("newsletter.invalidEmail")).toBe(
      "Introduce una dirección de email válida.",
    );
    expect(createTranslator("ja")("newsletter.unavailable")).not.toBe(en["newsletter.unavailable"]);
    expect(createTranslator("fr")("newsletter.network")).not.toBe(en["newsletter.network"]);
    expect(createTranslator("es")("demo.suggestionLabel", { agent: "{agent}<Swarmie>" })).toBe(
      "Propuesto por {agent}<Swarmie>",
    );
  });
});

/** Type assertions protect new catalog consumers without executing invalid calls. */
function translationTypes() {
  const t = createTranslator("en");
  // @ts-expect-error A translated sentence must be a known catalog key.
  t("missing.key");
  // @ts-expect-error The Agent name is required for an attributed suggestion.
  t("demo.suggestionLabel");
  // @ts-expect-error Pure text messages accept no interpolation arguments.
  t("common.language", { language: "en" });
}
void translationTypes;
