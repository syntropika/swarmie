import * as Application from "expo-application";
import * as Device from "expo-device";
import { ScrollView, Text } from "react-native";

import { useConnection } from "~/src/connection/provider";
import { useLocale } from "~/src/platform/i18n/provider";
import { LanguageSettings } from "~/src/platform/i18n/settings";
import { Action, Page } from "~/src/ui/controls";

export default function SettingsScreen() {
  const { t } = useLocale();
  const { disconnect, pending } = useConnection();
  return (
    <Page>
      <ScrollView contentContainerClassName="gap-6 p-6">
        <Text className="text-xl font-semibold text-ink">{t("settings.connected")}</Text>
        <Text className="text-base leading-7 text-text-muted">{t("settings.disconnectIntro")}</Text>
        <Action label={t("settings.disconnect")} onPress={disconnect} disabled={pending} />
        <Text className="text-sm text-text-muted">
          {t("settings.version", {
            version: Application.nativeApplicationVersion ?? t("settings.development"),
            os: Device.osName ?? "",
            osVersion: Device.osVersion ?? "",
          })}
        </Text>
        <Text className="text-sm leading-6 text-text-muted">{t("settings.privacy")}</Text>
        <LanguageSettings />
      </ScrollView>
    </Page>
  );
}
