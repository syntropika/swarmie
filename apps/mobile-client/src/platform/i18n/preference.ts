import { isLocale } from "~/src/platform/i18n/locale";
import type { LocalePreference } from "~/src/platform/i18n/locale";

type Storage = { read: () => Promise<string | null>; write: (value: string) => Promise<void> };
type Snapshot = { preference: LocalePreference; persistenceAvailable: boolean };

/** User selection wins over a late restore; writes retain order and failure keeps the in-memory choice. */
export function createLocalePreference(storage: Storage) {
  let snapshot: Snapshot = { preference: "automatic", persistenceAvailable: true };
  let selected = false;
  let writes = Promise.resolve();
  const listeners = new Set<() => void>();
  function update(next: Snapshot) {
    snapshot = next;
    for (const listener of listeners) {
      listener();
    }
  }
  return {
    getSnapshot: () => snapshot,
    subscribe: (listener: () => void) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    restore: async () => {
      try {
        const value = await storage.read();
        if (!selected && (value === "automatic" || isLocale(value))) {
          update({ ...snapshot, preference: value });
        }
      } catch {
        update({ ...snapshot, persistenceAvailable: false });
      }
    },
    setPreference: (preference: LocalePreference) => {
      selected = true;
      update({ ...snapshot, preference });
      writes = writes
        .then(async () => {
          await storage.write(preference);
          update({ ...snapshot, persistenceAvailable: true });
        })
        .catch(() => {
          update({ ...snapshot, persistenceAvailable: false });
        });
    },
  };
}
