import { describe, expect, test } from "bun:test";

import { AccountAccessError } from "~/src/account/contracts";
import { accountErrorKey } from "~/src/account/flow";
import { en } from "~/src/platform/i18n/catalogs/en";
import { detectLocale, isLocale, locales, matchLocale } from "~/src/platform/i18n/locale";
import { createLocalePreference } from "~/src/platform/i18n/preference";
import { catalogs, createTranslator, statusLabel } from "~/src/platform/i18n/translate";

function placeholders(value: string) {
  return [...value.matchAll(/\{(?<name>[^}]+)\}/gu)]
    .map((match) => match[1])
    .toSorted((a, b) => String(a).localeCompare(String(b), "en"));
}

describe("mobile localization", () => {
  test("the supported language set matches the web and every catalog has complete keys and slots", () => {
    expect(locales).toEqual(["en", "es", "fr", "de", "pt", "it", "ja", "zh-CN"]);
    for (const locale of locales) {
      const catalog = catalogs[locale];
      expect(Object.keys(catalog).toSorted()).toEqual(Object.keys(en).toSorted());
      for (const [key, value] of Object.entries(en)) {
        const translated = Object.entries(catalog).find(([candidate]) => candidate === key)?.[1];
        expect(typeof translated).toBe("string");
        expect(translated?.trim().length).toBeGreaterThan(0);
        expect(placeholders(translated ?? "")).toEqual(placeholders(value));
      }
    }
  });

  test("device priority, regional matching, Chinese scripts, and English fallback are deterministic", () => {
    expect(detectLocale(["nl-NL", "es-MX", "de-DE"])).toBe("es");
    expect(detectLocale(["pt-BR"])).toBe("pt");
    expect(detectLocale(["fr_CA"])).toBe("fr");
    expect(detectLocale(["en-US-u-ca-gregory", "es"])).toBe("en");
    for (const tag of ["zh", "zh-CN", "zh-SG", "zh-Hans-CN"]) {
      expect(matchLocale(tag)).toBe("zh-CN");
    }
    for (const tag of [
      "zh-TW",
      "zh-Hant",
      "zh-Hant-CN",
      "zh-CN-Hant",
      "zh-HK",
      "es-random-invalid!",
    ]) {
      expect(matchLocale(tag)).toBeUndefined();
    }
    expect(detectLocale(["zh-Hant", "ja-JP"])).toBe("ja");
    expect(detectLocale(["ar", "zz"])).toBe("en");
    expect(detectLocale([])).toBe("en");
    expect(isLocale("zh-cn")).toBe(false);
  });

  test("interpolation keeps user values as text and localized failures exclude raw service errors", () => {
    const t = createTranslator("es");
    expect(t("routines.every", { seconds: "1.000" })).toBe("Intervalo: 1.000 s");
    expect(
      t("preview.guide", { email: "<user>{code}", password: "example", code: "123456" }),
    ).toContain("<user>{code}");
    expect(t(accountErrorKey(new AccountAccessError("cancelled")))).toContain("cancelado");
    expect(t(accountErrorKey(new Error("private-secret")))).not.toContain("private-secret");
    expect(statusLabel(t, "needs_input")).toBe("Necesita una respuesta");
    expect(statusLabel(t, "private-runtime-state")).toBe("Estado desconocido");
  });

  test("explicit selection wins over a late stored preference", async () => {
    const read = Promise.withResolvers<string | null>();
    const preference = createLocalePreference({
      read: async () => await read.promise,
      write: async () => {},
    });
    const restore = preference.restore();
    preference.setPreference("ja");
    read.resolve("es");
    await restore;
    expect(preference.getSnapshot().preference).toBe("ja");
  });

  test("failed storage preserves selection and reports session-only persistence", async () => {
    const degraded = Promise.withResolvers<undefined>();
    const preference = createLocalePreference({
      read: async () => null,
      write: async () => {
        throw new Error("storage unavailable");
      },
    });
    const unsubscribe = preference.subscribe(() => {
      if (!preference.getSnapshot().persistenceAvailable) {
        degraded.resolve(undefined);
      }
    });
    preference.setPreference("es");
    await degraded.promise;
    expect(preference.getSnapshot()).toEqual({ preference: "es", persistenceAvailable: false });
    unsubscribe();
  });

  test("preference writes remain ordered and invalid saved values fall back to device language", async () => {
    const firstStarted = Promise.withResolvers<undefined>();
    const gate = Promise.withResolvers<undefined>();
    const finished = Promise.withResolvers<undefined>();
    const saved: string[] = [];
    const preference = createLocalePreference({
      read: async () => "unsupported",
      write: async (value) => {
        saved.push(value);
        if (value === "es") {
          firstStarted.resolve(undefined);
          await gate.promise;
        } else {
          finished.resolve(undefined);
        }
      },
    });
    await preference.restore();
    expect(preference.getSnapshot().preference).toBe("automatic");
    preference.setPreference("es");
    preference.setPreference("ja");
    await firstStarted.promise;
    expect(saved).toEqual(["es"]);
    gate.resolve(undefined);
    await finished.promise;
    expect(saved).toEqual(["es", "ja"]);
    expect(preference.getSnapshot().preference).toBe("ja");
  });
});
