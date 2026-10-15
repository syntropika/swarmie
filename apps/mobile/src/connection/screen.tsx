import { useState } from "react";
import { ScrollView, Text, View } from "react-native";

import { useConnection } from "~/src/connection/provider";
import { connectionSchema } from "~/src/connection/settings";
import { useLocale } from "~/src/platform/i18n/provider";
import type { Translate } from "~/src/platform/i18n/translate";
import { Action, Field, Notice, Page } from "~/src/ui/controls";

export function ConnectScreen() {
  const { t } = useLocale();
  const { connect, pending, error, restoring } = useConnection();
  const [url, setUrl] = useState("");
  const [token, setToken] = useState("");
  const parsed = connectionSchema.safeParse({ url: url.trim(), token });
  return (
    <Page>
      <ScrollView
        keyboardShouldPersistTaps="handled"
        contentContainerClassName="grow justify-center gap-6 px-6 py-12"
      >
        <View className="gap-3">
          <Text accessibilityRole="header" className="text-4xl font-semibold text-ink">
            Swarmie
          </Text>
          <Text className="text-xl text-text-muted">{t("connection.tagline")}</Text>
        </View>
        <Text className="text-base leading-7 text-ink">{t("connection.intro")}</Text>
        <Field
          label={t("connection.url")}
          value={url}
          onChangeText={setUrl}
          autoCapitalize="none"
          autoCorrect={false}
          keyboardType="url"
          placeholder="https://runtime.example.com"
        />
        <Field
          label={t("connection.credential")}
          value={token}
          onChangeText={setToken}
          autoCapitalize="none"
          autoCorrect={false}
          secureTextEntry
        />
        <Text className="text-sm leading-6 text-text-muted">{t("connection.storage")}</Text>
        <Action
          label={connectionLabel(restoring, pending, t)}
          selected
          disabled={!parsed.success || pending || restoring}
          onPress={() => {
            if (parsed.success) {
              connect(parsed.data);
            }
          }}
        />
        {error === null ? null : <Notice>{t(error)}</Notice>}
      </ScrollView>
    </Page>
  );
}

function connectionLabel(restoring: boolean, pending: boolean, t: Translate) {
  if (restoring) {
    return t("connection.restoring");
  }
  return pending ? t("connection.connecting") : t("connection.connect");
}
