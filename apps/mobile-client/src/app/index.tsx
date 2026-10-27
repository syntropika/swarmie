import { NativeTextAction } from "@swarmie/ui-native";
import { useState } from "react";
import { View } from "react-native";

import { unavailableAccountAccess } from "~/src/account/contracts";
import { AccountScreen } from "~/src/account/screen";
import { ConnectScreen } from "~/src/connection/screen";
import { useLocale } from "~/src/platform/i18n/provider";

export default function HomeScreen() {
  const { t } = useLocale();
  const [runtimeStatus, setRuntimeStatus] = useState(false);
  if (__DEV__ && runtimeStatus) {
    return (
      <View className="flex-1">
        <ConnectScreen />
        <NativeTextAction
          label={t("nav.backAccount")}
          onPress={() => {
            setRuntimeStatus(false);
          }}
        />
      </View>
    );
  }
  return (
    <View className="flex-1 bg-native-canvas">
      <AccountScreen access={unavailableAccountAccess} />
      {__DEV__ ? (
        <View className="items-center pb-2">
          <NativeTextAction
            label={t("nav.development")}
            onPress={() => {
              setRuntimeStatus(true);
            }}
          />
        </View>
      ) : null}
    </View>
  );
}
