import { NativeTextAction } from "@swarmie/ui-native";
import { useMemo, useState } from "react";
import { Modal, ScrollView, Text, View } from "react-native";
import { useReducedMotion } from "react-native-reanimated";

import { createPreviewAccess } from "~/src/account/preview-access";
import { AccountScreen } from "~/src/account/screen";
import { useLocale } from "~/src/platform/i18n/provider";
import { LanguageSettings } from "~/src/platform/i18n/settings";
import { SafeView } from "~/src/ui/controls";

function Preview() {
  const { t } = useLocale();
  const access = useMemo(() => createPreviewAccess(), []);
  const [guide, setGuide] = useState(false);
  const reducedMotion = useReducedMotion();
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
        animationType={reducedMotion ? "none" : "fade"}
        onRequestClose={() => {
          setGuide(false);
        }}
      >
        <SafeView edges={["top", "bottom", "left", "right"]} className="flex-1 bg-native-canvas">
          <ScrollView
            className="flex-1 bg-native-canvas"
            contentContainerClassName="grow justify-center gap-6 p-8"
          >
            <Text
              accessibilityRole="header"
              className="font-native text-2xl font-semibold text-native-ink"
            >
              {t("preview.title")}
            </Text>
            <Text className="font-native text-base leading-7 text-native-muted">
              {t("preview.guide", {
                email: "alex@example.com",
                password: "preview-password",
                code: "123456",
              })}
            </Text>
            <Text className="font-native text-base leading-7 text-native-muted">
              {t("preview.notice")}
            </Text>
            <LanguageSettings />
            <NativeTextAction
              label={t("preview.close")}
              onPress={() => {
                setGuide(false);
              }}
            />
          </ScrollView>
        </SafeView>
      </Modal>
    </View>
  );
}

export default function OnboardingPreview() {
  const { t } = useLocale();
  if (!__DEV__ && process.env.EXPO_PUBLIC_AUTH_PREVIEW !== "1") {
    return (
      <View className="flex-1 justify-center bg-native-canvas p-8">
        <Text className="font-native text-base text-native-ink">{t("preview.unavailable")}</Text>
      </View>
    );
  }
  return <Preview />;
}
