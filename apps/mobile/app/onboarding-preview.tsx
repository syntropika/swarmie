import { NativeTextAction } from "@swarmie/ui-native";
import { useMemo, useState } from "react";
import { Modal, Text, View } from "react-native";

import { createPreviewAccess } from "../src/account/preview-access";
import { AccountScreen } from "../src/account/screen";

function Preview() {
  const access = useMemo(() => createPreviewAccess(), []);
  const [guide, setGuide] = useState(false);
  return (
    <View className="flex-1 bg-native-canvas">
      <View className="items-center px-6 pt-2">
        <NativeTextAction
          label="Interactive preview · No real accounts"
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
        <View className="flex-1 justify-center gap-6 bg-native-canvas p-8">
          <Text accessibilityRole="header" className="text-2xl font-semibold text-native-ink">
            Preview guide
          </Text>
          <Text className="text-base leading-7 text-native-muted">
            Use alex@example.com for an existing account, or any other example email for a new
            account. Password: preview-password. Verification code: 123456.
          </Text>
          <Text className="text-base leading-7 text-native-muted">
            Use example credentials only. This preview sends no email, creates no account and does
            not invoke device authentication.
          </Text>
          <NativeTextAction
            label="Close guide"
            onPress={() => {
              setGuide(false);
            }}
          />
        </View>
      </Modal>
    </View>
  );
}

export default function OnboardingPreview() {
  if (!__DEV__ && process.env.EXPO_PUBLIC_AUTH_PREVIEW !== "1") {
    return (
      <View className="flex-1 justify-center bg-native-canvas p-8">
        <Text className="text-base text-native-ink">This preview is not available.</Text>
      </View>
    );
  }
  return <Preview />;
}
