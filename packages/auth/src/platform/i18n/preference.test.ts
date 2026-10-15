import { describe, expect, test } from "bun:test";

import {
  createLocalePreferenceStore,
  initialLocaleSnapshot,
  isLocaleStorageEvent,
  localeStorageKey,
} from "~/src/platform/i18n/preference";
import type { LocalePreference } from "~/src/platform/i18n/preference";

function browserPreference(value: unknown = null) {
  const storageListeners = new Set<(next: unknown) => void>();
  const languageListeners = new Set<() => void>();
  const browser = {
    value,
    languages: ["nl-NL", "es-MX", "en-US"],
    readBlocked: false,
    writeBlocked: false,
    environment: {
      read: () => {
        if (browser.readBlocked) {
          throw new Error("Storage unavailable");
        }
        return browser.value;
      },
      write: (preference: LocalePreference) => {
        if (browser.writeBlocked) {
          throw new Error("Storage unavailable");
        }
        browser.value = preference;
      },
      languages: () => browser.languages,
      listenStorage: (listener: (next: unknown) => void) => {
        storageListeners.add(listener);
        return () => {
          storageListeners.delete(listener);
        };
      },
      listenLanguage: (listener: () => void) => {
        languageListeners.add(listener);
        return () => {
          languageListeners.delete(listener);
        };
      },
    },
    crossTab: (next: unknown) => {
      browser.value = next;
      for (const listener of storageListeners) {
        listener(next);
      }
    },
    changeLanguages: (languages: string[]) => {
      browser.languages = languages;
      for (const listener of languageListeners) {
        listener();
      }
    },
    listeners: () => storageListeners.size + languageListeners.size,
  };
  return browser;
}

describe("browser-local language preference", () => {
  test("hydrates in English, then automatic follows ordered languages; explicit choices survive browser changes", () => {
    expect(initialLocaleSnapshot).toEqual({ locale: "en", preference: "automatic" });
    const browser = browserPreference();
    const store = createLocalePreferenceStore(browser.environment);
    const stop = store.subscribe(() => {});
    expect(store.getSnapshot()).toEqual({ locale: "es", preference: "automatic" });
    browser.changeLanguages(["zh-Hant-TW", "de-AT"]);
    expect(store.getSnapshot().locale).toBe("de");
    store.setPreference("fr");
    const explicit = store.getSnapshot();
    browser.changeLanguages(["ja-JP"]);
    expect(store.getSnapshot()).toBe(explicit);
    expect(createLocalePreferenceStore(browser.environment).getSnapshot().locale).toBe("fr");
    store.setPreference("automatic");
    expect(store.getSnapshot()).toEqual({ locale: "ja", preference: "automatic" });
    stop();
  });

  test("validates stored and cross-tab values, accepts supported locales, and resets cleared preferences", () => {
    const browser = browserPreference("__proto__");
    const store = createLocalePreferenceStore(browser.environment);
    const stop = store.subscribe(() => {});
    expect(store.getSnapshot().preference).toBe("automatic");
    browser.crossTab("zh-CN");
    expect(store.getSnapshot()).toEqual({ locale: "zh-CN", preference: "zh-CN" });
    browser.crossTab({ locale: "it" });
    expect(store.getSnapshot().locale).toBe("es");
    browser.crossTab("it");
    browser.crossTab(null);
    expect(store.getSnapshot()).toEqual({ locale: "es", preference: "automatic" });
    stop();
  });

  test("blocked persistence retains the local choice across subscriber teardown and resumes cross-tab synchronization", () => {
    const browser = browserPreference("de");
    browser.writeBlocked = true;
    const store = createLocalePreferenceStore(browser.environment);
    let notifications = 0;
    const receive = () => {
      notifications += 1;
    };
    const stop = store.subscribe(receive);
    store.setPreference("pt");
    stop();
    expect(browser.listeners()).toBe(0);
    browser.changeLanguages(["ja"]);
    expect(notifications).toBe(1);
    const stopAgain = store.subscribe(receive);
    expect(store.getSnapshot().locale).toBe("pt");
    browser.crossTab("en");
    expect(store.getSnapshot().locale).toBe("en");
    stopAgain();
  });

  test("blocked reads fall back safely and refresh automatic languages after resubscribing", () => {
    const browser = browserPreference("it");
    browser.readBlocked = true;
    browser.languages = [];
    const store = createLocalePreferenceStore(browser.environment);
    expect(store.getSnapshot()).toEqual(initialLocaleSnapshot);
    const stop = store.subscribe(() => {});
    stop();
    browser.languages = ["fr-CA"];
    const stopAgain = store.subscribe(() => {});
    expect(store.getSnapshot().locale).toBe("fr");
    stopAgain();
  });

  test("only localStorage events for the preference key or a complete clear can synchronize", () => {
    const local: Storage = {
      length: 0,
      clear: () => {},
      getItem: () => null,
      key: () => null,
      removeItem: () => {},
      setItem: () => {},
    };
    const session: Storage = {
      length: 0,
      clear: () => {},
      getItem: () => null,
      key: () => null,
      removeItem: () => {},
      setItem: () => {},
    };
    expect(isLocaleStorageEvent({ key: localeStorageKey, storageArea: local }, local)).toBe(true);
    expect(isLocaleStorageEvent({ key: null, storageArea: local }, local)).toBe(true);
    expect(isLocaleStorageEvent({ key: "swarmie.theme", storageArea: local }, local)).toBe(false);
    expect(isLocaleStorageEvent({ key: localeStorageKey, storageArea: session }, local)).toBe(
      false,
    );
    expect(isLocaleStorageEvent({ key: localeStorageKey, storageArea: null }, local)).toBe(false);
  });
});
