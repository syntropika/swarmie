import { NativeProvider } from "@swarmie/ui-native";

import "~/src/global.css";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { Stack } from "expo-router";
import { StatusBar } from "expo-status-bar";
import { useMemo } from "react";
import { GestureHandlerRootView } from "react-native-gesture-handler";
import { KeyboardProvider } from "react-native-keyboard-controller";
import { SafeAreaProvider } from "react-native-safe-area-context";
import { withUniwind } from "uniwind";

import { LocaleProvider } from "~/src/platform/i18n/provider";

const GestureRoot = withUniwind(GestureHandlerRootView);

export default function RootLayout() {
  const cache = useMemo(
    () => new QueryClient({ defaultOptions: { mutations: { retry: false } } }),
    [],
  );
  // Expo's status-bar appearance is a semantic enum, not a component stylesheet.
  const appearance = { style: "dark" as const };
  return (
    <LocaleProvider>
      <GestureRoot className="flex-1">
        <SafeAreaProvider>
          <NativeProvider>
            <KeyboardProvider>
              <QueryClientProvider client={cache}>
                <StatusBar {...appearance} />
                <Stack screenOptions={{ headerShown: false }} />
              </QueryClientProvider>
            </KeyboardProvider>
          </NativeProvider>
        </SafeAreaProvider>
      </GestureRoot>
    </LocaleProvider>
  );
}
