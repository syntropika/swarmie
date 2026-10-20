import type { Preview } from "@storybook/react-vite";
import { GestureHandlerRootView } from "react-native-gesture-handler";
import { SafeAreaProvider } from "react-native-safe-area-context";
import { withUniwind } from "uniwind";

import { NativeProvider } from "../src/provider";

import "../src/styles.css";

const GestureRoot = withUniwind(GestureHandlerRootView);

const preview: Preview = {
  tags: ["autodocs"],
  decorators: [
    (Story) => (
      <GestureRoot className="min-h-screen bg-native-canvas">
        <SafeAreaProvider>
          <NativeProvider>
            <Story />
          </NativeProvider>
        </SafeAreaProvider>
      </GestureRoot>
    ),
  ],
  parameters: { layout: "padded", a11y: { test: "error" } },
};

export default preview;
