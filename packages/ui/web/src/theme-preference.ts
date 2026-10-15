/** A browser preference; System delegates its resolved colors to native color-scheme CSS. */
export type ThemePreference = "light" | "dark" | "system";

export const THEME_PREFERENCE_STORAGE_KEY = "swarmie.theme";

/** Only these current values are accepted from storage or native controls. */
export function isThemePreference(value: unknown): value is ThemePreference {
  return value === "light" || value === "dark" || value === "system";
}

/** Trusted static code runs before first paint and hydration; no user data becomes script. */
export const THEME_PREFERENCE_BOOTSTRAP = `(()=>{let theme="system";try{const stored=localStorage.getItem("${THEME_PREFERENCE_STORAGE_KEY}");if(stored==="light"||stored==="dark"||stored==="system")theme=stored;}catch{theme="system";}document.documentElement.dataset.theme=theme;})();`;

type ThemePreferenceEnvironment = {
  read: () => unknown;
  write: (preference: ThemePreference) => void;
  apply: (preference: ThemePreference) => void;
  listen: (receive: (value: unknown) => void) => () => void;
};

/** The small adapter contract lets browsers retain in-memory choices when storage is blocked. */
export function createThemePreferenceStore(environment: ThemePreferenceEnvironment) {
  function readPreference(fallback: ThemePreference): ThemePreference {
    try {
      const stored = environment.read();
      return isThemePreference(stored) ? stored : "system";
    } catch {
      // Privacy settings can block storage; keep this tab's current selection usable.
      return fallback;
    }
  }

  let preference = readPreference("system");
  let hasUnpersistedChoice = false;
  const subscribers = new Set<() => void>();
  let stopListening: (() => void) | undefined;

  function update(next: ThemePreference) {
    environment.apply(next);
    if (preference === next) {
      return;
    }
    preference = next;
    for (const notify of subscribers) {
      notify();
    }
  }

  return {
    getSnapshot: () => preference,
    subscribe(notify: () => void) {
      subscribers.add(notify);
      if (subscribers.size === 1) {
        stopListening = environment.listen((value) => {
          hasUnpersistedChoice = false;
          update(isThemePreference(value) ? value : "system");
        });
        update(hasUnpersistedChoice ? preference : readPreference(preference));
      }
      return () => {
        subscribers.delete(notify);
        if (subscribers.size === 0) {
          stopListening?.();
          stopListening = undefined;
        }
      };
    },
    setPreference(next: ThemePreference) {
      try {
        environment.write(next);
      } catch {
        // A full or blocked store cannot prevent a choice from applying in this tab.
        hasUnpersistedChoice = true;
        update(next);
        return;
      }
      hasUnpersistedChoice = false;
      update(next);
    },
  };
}

function createBrowserThemePreferenceStore() {
  return createThemePreferenceStore({
    read: () => window.localStorage.getItem(THEME_PREFERENCE_STORAGE_KEY),
    write: (preference) => {
      window.localStorage.setItem(THEME_PREFERENCE_STORAGE_KEY, preference);
    },
    apply: (preference) => {
      document.documentElement.dataset.theme = preference;
    },
    listen: (receive) => {
      const onStorage = (event: StorageEvent) => {
        if (event.key !== null && event.key !== THEME_PREFERENCE_STORAGE_KEY) {
          return;
        }
        let source: Storage;
        try {
          source = window.localStorage;
        } catch {
          // Privacy settings may have changed since subscribing; retain this tab's choice.
          return;
        }
        // Real storage events identify their source; ignore sessionStorage changes.
        if (event.storageArea === source) {
          receive(event.newValue);
        }
      };
      window.addEventListener("storage", onStorage);
      return () => {
        window.removeEventListener("storage", onStorage);
      };
    },
  });
}

// No user state is created on the SSR server; this instance exists only in a browser tab.
let browserStore: ReturnType<typeof createThemePreferenceStore> | undefined;

function getBrowserStore() {
  browserStore ??= createBrowserThemePreferenceStore();
  return browserStore;
}

export function getThemePreferenceSnapshot(): ThemePreference {
  return typeof window === "undefined" ? "system" : getBrowserStore().getSnapshot();
}

/** The same default keeps SSR and the first hydration render consistent. */
export function getServerThemePreferenceSnapshot(): ThemePreference {
  return "system";
}

export function subscribeThemePreference(notify: () => void): () => void {
  return getBrowserStore().subscribe(notify);
}

export function setThemePreference(preference: ThemePreference): void {
  getBrowserStore().setPreference(preference);
}
