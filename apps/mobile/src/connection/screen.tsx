import { useState } from "react";
import { ScrollView, Text, View } from "react-native";

import { Action, Field, Notice, Page } from "../ui/controls";
import { useConnection } from "./provider";
import { connectionSchema } from "./settings";

export function ConnectScreen() {
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
          <Text className="text-xl text-text-muted">Your agents, wherever you are.</Text>
        </View>
        <Text className="text-base leading-7 text-ink">
          Connect to your running Swarmie runtime. Conversations and routines stay on that runtime
          when you close the app.
        </Text>
        <Field
          label="Runtime URL"
          value={url}
          onChangeText={setUrl}
          autoCapitalize="none"
          autoCorrect={false}
          keyboardType="url"
          placeholder="https://runtime.example.com"
        />
        <Field
          label="Device credential"
          value={token}
          onChangeText={setToken}
          autoCapitalize="none"
          autoCorrect={false}
          secureTextEntry
        />
        <Text className="text-sm leading-6 text-text-muted">
          Use the HTTPS endpoint and credential provided by your runtime administrator. The
          credential stays in this device’s secure storage.
        </Text>
        <Action
          label={connectionLabel(restoring, pending)}
          selected
          disabled={!parsed.success || pending || restoring}
          onPress={() => {
            if (parsed.success) {
              connect(parsed.data);
            }
          }}
        />
        {error === null ? null : <Notice>{error}</Notice>}
      </ScrollView>
    </Page>
  );
}

function connectionLabel(restoring: boolean, pending: boolean) {
  if (restoring) {
    return "Restoring connection…";
  }
  return pending ? "Connecting…" : "Connect runtime";
}
