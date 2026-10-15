import { ThemePreferenceControl } from "@swarmie/ui-web";

import { useLocale } from "~/src/platform/i18n/provider";
import { useThemePreference } from "~/src/platform/theme/use-theme-preference";

/** Consumer-owned localization and browser state compose the neutral shared picker. */
export function ThemeControl({ layout = "stacked" }: { layout?: "stacked" | "row" }) {
  const { t } = useLocale();
  const { preference, setPreference } = useThemePreference();
  return (
    <ThemePreferenceControl
      layout={layout}
      value={preference}
      onValueChange={setPreference}
      label={t("theme.label")}
      lightLabel={t("theme.light")}
      darkLabel={t("theme.dark")}
      systemLabel={t("theme.system")}
    />
  );
}
