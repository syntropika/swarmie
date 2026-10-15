import {
  getServerThemePreferenceSnapshot,
  getThemePreferenceSnapshot,
  setThemePreference,
  subscribeThemePreference,
} from "@swarmie/ui-web";
import { useSyncExternalStore } from "react";

/** Hydrate from System, then expose this tab's validated browser preference. */
export function useThemePreference() {
  const preference = useSyncExternalStore(
    subscribeThemePreference,
    getThemePreferenceSnapshot,
    getServerThemePreferenceSnapshot,
  );
  return { preference, setPreference: setThemePreference };
}
