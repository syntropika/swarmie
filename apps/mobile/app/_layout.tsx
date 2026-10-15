import "../global.css";
import { NativeProvider, NativeTextAction } from "@swarmie/ui-native";
import { Stack, usePathname } from "expo-router";
import { StatusBar } from "expo-status-bar";
import { useState } from "react";
import { View } from "react-native";
import { GestureHandlerRootView } from "react-native-gesture-handler";
import { KeyboardProvider } from "react-native-keyboard-controller";
import { SafeAreaProvider } from "react-native-safe-area-context";
import { withUniwind } from "uniwind";

import { unavailableAccountAccess } from "../src/account/contracts";
import { AccountScreen } from "../src/account/screen";
import { RuntimeProvider, useConnection } from "../src/connection/provider";
import { ConnectScreen } from "../src/connection/screen";

const GestureRoot = withUniwind(GestureHandlerRootView);

function Navigation() {
  const { connection } = useConnection();
  const pathname = usePathname();
  const [developmentConnection, setDevelopmentConnection] = useState(false);
  if (pathname === "/onboarding-preview") {
    return <Stack screenOptions={{ headerShown: false }} />;
  }
  if (!connection) {
    if (__DEV__ && developmentConnection) {
      return (
        <View className="flex-1">
          <ConnectScreen />
          <NativeTextAction
            label="Back to account sign-in"
            onPress={() => {
              setDevelopmentConnection(false);
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
              label="Development: connect a runtime"
              onPress={() => {
                setDevelopmentConnection(true);
              }}
            />
          </View>
        ) : null}
      </View>
    );
  }
  return (
    <Stack>
      <Stack.Screen name="(tabs)" options={{ headerShown: false }} />
      <Stack.Screen name="conversation/[id]" options={{ title: "Conversation" }} />
    </Stack>
  );
}

export default function RootLayout() {
  // Expo's status-bar appearance is a semantic enum, not a component stylesheet.
  const appearance = { style: "dark" as const };
  return (
    <GestureRoot className="flex-1">
      <SafeAreaProvider>
        <NativeProvider>
          <KeyboardProvider>
            <RuntimeProvider>
              <StatusBar {...appearance} />
              <Navigation />
            </RuntimeProvider>
          </KeyboardProvider>
        </NativeProvider>
      </SafeAreaProvider>
    </GestureRoot>
  );
}
