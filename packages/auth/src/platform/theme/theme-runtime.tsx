import { useThemePreference } from "~/src/platform/theme/use-theme-preference";

/** Keep the document synchronized even on routes without a preference picker. */
export function ThemeRuntime() {
  useThemePreference();
  return null;
}
