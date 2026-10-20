import { NativeBrand, NativeTextAction } from "@swarmie/ui-native";
import type { ReactNode } from "react";
import { KeyboardAvoidingView, Platform, ScrollView, Text, View } from "react-native";

import brandSource from "~/assets/swarmie-mono-ink.png";
import { useLocale } from "~/src/platform/i18n/provider";
import { SafeView } from "~/src/ui/controls";

export function AccountFrame({
  title,
  subtitle,
  email,
  onChangeEmail,
  children,
  footer,
}: {
  title: string;
  subtitle?: string | undefined;
  email?: string | undefined;
  onChangeEmail?: (() => void) | undefined;
  children: ReactNode;
  footer: ReactNode;
}) {
  const { t } = useLocale();
  return (
    <SafeView edges={["top", "bottom", "left", "right"]} className="flex-1 bg-native-canvas">
      <KeyboardAvoidingView
        className="flex-1"
        behavior={Platform.OS === "ios" ? "padding" : "height"}
      >
        <ScrollView
          keyboardShouldPersistTaps="handled"
          keyboardDismissMode="on-drag"
          contentContainerClassName="grow"
        >
          <View className="mx-auto w-full max-w-native-content grow px-7 pt-7 pb-5 sm:px-10">
            <NativeBrand source={brandSource} />
            <View className="gap-4 pt-14">
              <Text
                accessibilityRole="header"
                className="font-native text-native-heading text-native-ink"
              >
                {title}
              </Text>
              {subtitle === undefined ? null : (
                <Text className="font-native text-base leading-6 text-native-muted">
                  {subtitle}
                </Text>
              )}
            </View>
            {email === undefined ? null : (
              <View className="mt-7 min-h-11 flex-row items-center gap-3">
                <Text className="min-w-0 flex-1 font-native text-base text-native-muted">
                  {email}
                </Text>
                {onChangeEmail ? (
                  <NativeTextAction
                    label={t("account.change")}
                    accessibilityHint={t("account.changeEmail")}
                    onPress={onChangeEmail}
                  />
                ) : null}
              </View>
            )}
            <View className="gap-5 pt-8">{children}</View>
            <View className="mt-auto gap-4 pt-12">{footer}</View>
          </View>
        </ScrollView>
      </KeyboardAvoidingView>
    </SafeView>
  );
}
