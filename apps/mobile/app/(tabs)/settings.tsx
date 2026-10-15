import * as Application from "expo-application";
import * as Device from "expo-device";
import { ScrollView, Text } from "react-native";

import { useConnection } from "../../src/connection/provider";
import { Action, Page } from "../../src/ui/controls";

export default function SettingsScreen() {
  const { disconnect, pending } = useConnection();
  return (
    <Page>
      <ScrollView contentContainerClassName="gap-6 p-6">
        <Text className="text-xl font-semibold text-ink">Connected to your runtime</Text>
        <Text className="text-base leading-7 text-text-muted">
          Disconnect removes this device’s stored credential and clears its in-memory runtime cache.
          Retained conversations, drafts and routines stay with their owners.
        </Text>
        <Action label="Disconnect runtime" onPress={disconnect} disabled={pending} />
        <Text className="text-sm text-text-muted">
          Swarmie {Application.nativeApplicationVersion ?? "development"} · {Device.osName}{" "}
          {Device.osVersion}
        </Text>
        <Text className="text-sm leading-6 text-text-muted">
          No analytics or diagnostic uploads. Updates are bundled with the app. Notifications and
          media permissions are not requested at startup.
        </Text>
      </ScrollView>
    </Page>
  );
}
