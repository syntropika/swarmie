import { NativeButton, NativeNotice } from "@swarmie/ui-native";
import { Text, View } from "react-native";

import { languageNames, locales } from "~/src/platform/i18n/locale";
import { useLocale } from "~/src/platform/i18n/provider";

export function LanguageSettings() {
  const { t, preference, setPreference, persistenceAvailable } = useLocale();
  return (
    <View className="gap-2">
      <Text accessibilityRole="header" className="text-xl font-semibold text-native-ink">
        {t("language.title")}
      </Text>
      <NativeButton
        label={t("language.automatic")}
        accessibilityState={{ selected: preference === "automatic" }}
        tone={preference === "automatic" ? "primary" : "secondary"}
        onPress={() => {
          setPreference("automatic");
        }}
      />
      {locales.map((locale) => (
        <NativeButton
          key={locale}
          label={languageNames[locale]}
          accessibilityLanguage={locale}
          accessibilityState={{ selected: preference === locale }}
          tone={preference === locale ? "primary" : "secondary"}
          onPress={() => {
            setPreference(locale);
          }}
        />
      ))}
      {persistenceAvailable ? null : <NativeNotice>{t("language.sessionOnly")}</NativeNotice>}
    </View>
  );
}
