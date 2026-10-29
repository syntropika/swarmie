import { describe, expect, test } from "bun:test";
import { runInNewContext } from "node:vm";

import {
  THEME_PREFERENCE_BOOTSTRAP,
  createThemePreferenceStore,
  getServerThemePreferenceSnapshot,
} from "./theme-preference";
import type { ThemePreference } from "./theme-preference";

function environment(initial: unknown = null) {
  let stored = initial;
  let applied: ThemePreference | undefined;
  let receive: ((value: unknown) => void) | undefined;
  let blocked = false;
  let writesBlocked = false;
  let listening = false;
  const adapter = {
    read: () => {
      if (blocked) {
        throw new Error("Storage unavailable");
      }
      return stored;
    },
    write: (preference: ThemePreference) => {
      if (blocked || writesBlocked) {
        throw new Error("Storage unavailable");
      }
      stored = preference;
    },
    apply: (preference: ThemePreference) => {
      applied = preference;
    },
    listen: (listener: (value: unknown) => void) => {
      receive = listener;
      listening = true;
      return () => {
        receive = undefined;
        listening = false;
      };
    },
  };
  return {
    adapter,
    stored: () => stored,
    applied: () => applied,
    listening: () => listening,
    block: () => {
      blocked = true;
    },
    blockWrites: () => {
      writesBlocked = true;
    },
    externalChange: (value: unknown) => {
      stored = value;
      receive?.(value);
    },
  };
}

describe("browser theme preference", () => {
  test("stored explicit choice remains explicit; System delegates resolution to CSS", () => {
    const browser = environment("dark");
    const store = createThemePreferenceStore(browser.adapter);
    const unsubscribe = store.subscribe(() => undefined);
    expect(browser.applied()).toBe("dark");
    store.setPreference("light");
    expect(store.getSnapshot()).toBe("light");
    expect(browser.stored()).toBe("light");
    expect(browser.applied()).toBe("light");
    store.setPreference("system");
    expect(browser.stored()).toBe("system");
    expect(browser.applied()).toBe("system");
    unsubscribe();
  });

  test("blocked storage preserves choices and resubscription in memory", () => {
    const browser = environment();
    browser.block();
    const store = createThemePreferenceStore(browser.adapter);
    expect(store.getSnapshot()).toBe("dark");
    const unsubscribe = store.subscribe(() => undefined);
    store.setPreference("dark");
    expect(store.getSnapshot()).toBe("dark");
    expect(browser.applied()).toBe("dark");
    unsubscribe();
    const resubscribe = store.subscribe(() => undefined);
    expect(store.getSnapshot()).toBe("dark");
    expect(browser.applied()).toBe("dark");
    resubscribe();
  });

  test("full storage keeps the latest in-memory choice even while the old value is readable", () => {
    const browser = environment("light");
    browser.blockWrites();
    const store = createThemePreferenceStore(browser.adapter);
    const unsubscribe = store.subscribe(() => undefined);
    store.setPreference("dark");
    expect(browser.stored()).toBe("light");
    expect(store.getSnapshot()).toBe("dark");
    unsubscribe();
    const resubscribe = store.subscribe(() => undefined);
    expect(browser.applied()).toBe("dark");
    expect(store.getSnapshot()).toBe("dark");
    resubscribe();
  });

  test("other tabs update all subscribers; malformed values and storage clear reset to Dark", () => {
    const browser = environment("light");
    const store = createThemePreferenceStore(browser.adapter);
    let firstNotifications = 0;
    let secondNotifications = 0;
    const unsubscribeFirst = store.subscribe(() => {
      firstNotifications += 1;
    });
    const unsubscribeSecond = store.subscribe(() => {
      secondNotifications += 1;
    });
    browser.externalChange("dark");
    expect(browser.applied()).toBe("dark");
    expect(store.getSnapshot()).toBe("dark");
    expect(firstNotifications).toBe(1);
    expect(secondNotifications).toBe(1);
    browser.externalChange("unsupported");
    expect(browser.applied()).toBe("dark");
    browser.externalChange("light");
    browser.externalChange(null);
    expect(store.getSnapshot()).toBe("dark");
    unsubscribeFirst();
    expect(browser.listening()).toBe(true);
    unsubscribeSecond();
    expect(browser.listening()).toBe(false);
  });

  test("invalid initial storage does not become a document theme", () => {
    const browser = environment({ theme: "dark" });
    const store = createThemePreferenceStore(browser.adapter);
    const unsubscribe = store.subscribe(() => undefined);
    expect(browser.applied()).toBe("dark");
    expect(getServerThemePreferenceSnapshot()).toBe("dark");
    unsubscribe();
  });
});

describe("theme bootstrap before paint", () => {
  test.each([
    ["light", "light"],
    ["dark", "dark"],
    ["system", "system"],
    [null, "dark"],
    ["DARK", "dark"],
    ['dark";throw new Error("Untrusted storage");', "dark"],
  ])("validates stored %p before applying %p", (stored, expected) => {
    const document = {
      documentElement: { dataset: { theme: "system" }, classList: { toggle: () => false } },
      querySelectorAll: () => [],
    };
    runInNewContext(THEME_PREFERENCE_BOOTSTRAP, {
      document,
      localStorage: { getItem: () => stored },
    });
    expect(document.documentElement.dataset.theme).toBe(expected);
  });

  test("unavailable storage leaves the pre-hydration document in Dark mode", () => {
    const document = {
      documentElement: { dataset: { theme: "system" }, classList: { toggle: () => false } },
      querySelectorAll: () => [],
    };
    runInNewContext(THEME_PREFERENCE_BOOTSTRAP, {
      document,
      localStorage: {
        getItem: () => {
          throw new Error("Storage unavailable");
        },
      },
    });
    expect(document.documentElement.dataset.theme).toBe("dark");
  });
});

test.each(["light", "dark", "system"] as const)(
  "theme bootstrap selects matching browser chrome for %s",
  (stored) => {
    const metadata = [
      { dataset: { swarmieTheme: "light" }, media: "" },
      { dataset: { swarmieTheme: "dark" }, media: "" },
    ];
    const document = {
      documentElement: { dataset: { theme: "system" }, classList: { toggle: () => false } },
      querySelectorAll: () => metadata,
    };
    runInNewContext(THEME_PREFERENCE_BOOTSTRAP, {
      document,
      localStorage: { getItem: () => stored },
    });
    expect(metadata.map((meta) => meta.media)).toEqual(
      stored === "system"
        ? ["(prefers-color-scheme: light)", "(prefers-color-scheme: dark)"]
        : [stored === "light" ? "all" : "not all", stored === "dark" ? "all" : "not all"],
    );
  },
);

test.each([
  ["light", true, "light"],
  ["dark", false, "dark"],
  ["system", false, "light"],
  ["system", true, "dark"],
] as const)(
  "stock theme classes resolve %s with dark preference %s",
  (stored, prefersDark, expected) => {
    const classes = new Set<string>();
    const document = {
      documentElement: {
        dataset: { theme: "system" },
        classList: {
          toggle: (name: string, enabled: boolean) => {
            if (enabled) {
              classes.add(name);
            } else {
              classes.delete(name);
            }
            return enabled;
          },
        },
      },
      querySelectorAll: () => [],
    };
    runInNewContext(THEME_PREFERENCE_BOOTSTRAP, {
      document,
      localStorage: { getItem: () => stored },
      matchMedia: () => ({ matches: prefersDark }),
    });
    expect([...classes]).toEqual([expected]);
    expect(document.documentElement.dataset.theme).toBe(stored);
  },
);
