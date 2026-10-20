import { ScrollView, Text, View } from "react-native";

import { useLocale } from "~/src/platform/i18n/provider";
import { Notice, Page } from "~/src/ui/controls";

export function ConnectScreen() {
  const { t } = useLocale();
  return (
    <Page>
      <ScrollView contentContainerClassName="grow justify-center gap-6 px-6 py-12">
        <View className="gap-3">
          <Text accessibilityRole="header" className="text-4xl font-semibold text-ink">
            Swarmie
          </Text>
          <Text className="text-xl text-text-muted">{t("connection.tagline")}</Text>
        </View>
        <Notice>{t("connection.unavailable")}</Notice>
      </ScrollView>
    </Page>
  );
}
