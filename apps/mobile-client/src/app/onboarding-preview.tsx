import { NativeTextAction } from "@swarmie/ui-native";
import { useMemo, useState } from "react";
import { Modal, ScrollView, Text, View } from "react-native";

import { createPreviewAccess } from "~/src/account/preview-access";
import { AccountScreen } from "~/src/account/screen";
import { useLocale } from "~/src/platform/i18n/provider";
import { LanguageSettings } from "~/src/platform/i18n/settings";

function Preview() {
  const { t } = useLocale();
  const access = useMemo(() => createPreviewAccess(), []);
  const [guide, setGuide] = useState(false);
  return (
    <View className="flex-1 bg-native-canvas">
      <View className="items-center px-6 pt-2">
        <NativeTextAction
          label={t("preview.banner")}
          onPress={() => {
            setGuide(true);
          }}
        />
      </View>
      <AccountScreen access={access} preview />
      <Modal
        visible={guide}
        animationType="fade"
        onRequestClose={() => {
          setGuide(false);
        }}
      >
        <ScrollView
          className="flex-1 bg-native-canvas"
          contentContainerClassName="grow justify-center gap-6 p-8"
        >
          <Text accessibilityRole="header" className="text-2xl font-semibold text-native-ink">
            {t("preview.title")}
          </Text>
          <Text className="text-base leading-7 text-native-muted">
            {t("preview.guide", {
              email: "alex@example.com",
              password: "preview-password",
              code: "123456",
            })}
          </Text>
          <Text className="text-base leading-7 text-native-muted">{t("preview.notice")}</Text>
          <LanguageSettings />
          <NativeTextAction
            label={t("preview.close")}
            onPress={() => {
              setGuide(false);
            }}
          />
        </ScrollView>
      </Modal>
    </View>
  );
}

export default function OnboardingPreview() {
  const { t } = useLocale();
  if (!__DEV__ && process.env.EXPO_PUBLIC_AUTH_PREVIEW !== "1") {
    return (
      <View className="flex-1 justify-center bg-native-canvas p-8">
        <Text className="text-base text-native-ink">{t("preview.unavailable")}</Text>
      </View>
    );
  }
  return <Preview />;
}
