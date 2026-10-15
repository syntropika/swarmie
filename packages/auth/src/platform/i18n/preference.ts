import { detectLocale, isLocale } from "~/src/platform/i18n/locale";
import type { Locale } from "~/src/platform/i18n/locale";

export type LocalePreference = "automatic" | Locale;

type LocaleSnapshot = Readonly<{ locale: Locale; preference: LocalePreference }>;

export const localeStorageKey = "swarmie.locale";
export const initialLocaleSnapshot: LocaleSnapshot = { locale: "en", preference: "automatic" };

export function isLocalePreference(value: unknown): value is LocalePreference {
  return value === "automatic" || isLocale(value);
}

type PreferenceEnvironment = {
  read: () => unknown;
  write: (value: LocalePreference) => void;
  languages: () => readonly string[];
  listenStorage: (receive: (value: unknown) => void) => () => void;
  listenLanguage: (notify: () => void) => () => void;
};

/** A browser preference remains usable in memory when persistence is blocked. */
export function createLocalePreferenceStore(environment: PreferenceEnvironment) {
  function readPreference(fallback: LocalePreference): LocalePreference {
    try {
      const value = environment.read();
      return isLocalePreference(value) ? value : "automatic";
    } catch {
      return fallback;
    }
  }
  function resolve(preference: LocalePreference): LocaleSnapshot {
    return {
      preference,
      locale: preference === "automatic" ? detectLocale(environment.languages()) : preference,
    };
  }
  let snapshot = resolve(readPreference("automatic"));
  let unpersisted = false;
  const subscribers = new Set<() => void>();
  let stopListening: (() => void) | undefined;
  function update(preference: LocalePreference) {
    const next = resolve(preference);
    if (snapshot.preference === next.preference && snapshot.locale === next.locale) {
      return;
    }
    snapshot = next;
    for (const subscriber of subscribers) {
      subscriber();
    }
  }
  return {
    getSnapshot: () => snapshot,
    subscribe: (subscriber: () => void) => {
      subscribers.add(subscriber);
      if (subscribers.size === 1) {
        const stopStorage = environment.listenStorage((value) => {
          unpersisted = false;
          update(isLocalePreference(value) ? value : "automatic");
        });
        const stopLanguage = environment.listenLanguage(() => {
          if (snapshot.preference === "automatic") {
            update("automatic");
          }
        });
        stopListening = () => {
          stopStorage();
          stopLanguage();
        };
        update(unpersisted ? snapshot.preference : readPreference(snapshot.preference));
      }
      return () => {
        subscribers.delete(subscriber);
        if (subscribers.size === 0) {
          stopListening?.();
          stopListening = undefined;
        }
      };
    },
    setPreference: (preference: LocalePreference) => {
      try {
        environment.write(preference);
        unpersisted = false;
      } catch {
        unpersisted = true;
      }
      update(preference);
    },
  };
}

/** Ignore unrelated keys and sessionStorage events before validating their contents. */
export function isLocaleStorageEvent(
  event: Pick<StorageEvent, "key" | "storageArea">,
  localStorage: Storage,
): boolean {
  return (
    event.storageArea === localStorage && (event.key === null || event.key === localeStorageKey)
  );
}

let browserStore: ReturnType<typeof createLocalePreferenceStore> | undefined;

function getBrowserStore() {
  browserStore ??= createLocalePreferenceStore({
    read: () => window.localStorage.getItem(localeStorageKey),
    write: (value) => {
      window.localStorage.setItem(localeStorageKey, value);
    },
    languages: () => navigator.languages,
    listenStorage: (receive) => {
      const listener = (event: StorageEvent) => {
        let storage: Storage;
        try {
          storage = window.localStorage;
        } catch {
          // A privacy setting may revoke access after the subscription was established.
          return;
        }
        if (isLocaleStorageEvent(event, storage)) {
          receive(event.newValue);
        }
      };
      window.addEventListener("storage", listener);
      return () => {
        window.removeEventListener("storage", listener);
      };
    },
    listenLanguage: (notify) => {
      window.addEventListener("languagechange", notify);
      return () => {
        window.removeEventListener("languagechange", notify);
      };
    },
  });
  return browserStore;
}

export function subscribeLocale(subscriber: () => void) {
  return getBrowserStore().subscribe(subscriber);
}

export function getLocaleSnapshot(): LocaleSnapshot {
  return typeof window === "undefined" ? initialLocaleSnapshot : getBrowserStore().getSnapshot();
}

export function getInitialLocaleSnapshot(): LocaleSnapshot {
  return initialLocaleSnapshot;
}

export function setLocalePreference(preference: LocalePreference) {
  getBrowserStore().setPreference(preference);
}
