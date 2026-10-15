import { Button, Icon, NavigationSidebar } from "@swarmie/ui-web";
import type { IconName } from "@swarmie/ui-web";
import type { ReactNode } from "react";

import { settingsSections } from "~/src/identity/session/navigation";
import type { SettingsSection } from "~/src/identity/session/navigation";
import type { PlainMessageKey } from "~/src/platform/i18n/catalogs/en";
import { useLocale } from "~/src/platform/i18n/provider";

export const settingsSectionLabels = {
  general: "settings.general",
  appearance: "settings.appearance",
  security: "settings.security",
  account: "settings.account",
} satisfies Record<SettingsSection, PlainMessageKey>;

const settingsSectionIcons = {
  general: "settings",
  appearance: "eye",
  security: "key",
  account: "people",
} satisfies Record<SettingsSection, IconName>;

/** The available settings sections replace conversation navigation inside the same app shell. */
export function SettingsSidebar({
  section,
  onSectionChange,
  navigationControl,
  closeNavigation,
}: {
  section: SettingsSection;
  onSectionChange: (section: SettingsSection) => void;
  navigationControl: ReactNode;
  closeNavigation: () => void;
}) {
  const { t } = useLocale();
  return (
    <NavigationSidebar
      label={t("settings.title")}
      header={
        <>
          <h2 className="min-w-0 flex-1 px-3 text-ui font-medium">{t("settings.title")}</h2>
          {navigationControl}
        </>
      }
    >
      <nav
        aria-label={t("settings.title")}
        className="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto"
      >
        {settingsSections.map((value) => (
          <Button
            key={value}
            variant="navigation"
            size="navigation"
            selected={section === value}
            aria-current={section === value ? "page" : undefined}
            onClick={() => {
              closeNavigation();
              onSectionChange(value);
            }}
          >
            <Icon name={settingsSectionIcons[value]} />
            {t(settingsSectionLabels[value])}
          </Button>
        ))}
      </nav>
    </NavigationSidebar>
  );
}
