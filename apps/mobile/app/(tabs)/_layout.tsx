import { Tabs } from "expo-router";
import type { ColorValue } from "react-native";
import { useCSSVariable } from "uniwind";

import { Icon } from "../../src/ui/icon";

export default function TabsLayout() {
  const tint = useCSSVariable("--color-primary");
  return (
    <Tabs screenOptions={typeof tint === "string" ? { tabBarActiveTintColor: tint } : {}}>
      <Tabs.Screen
        name="index"
        options={{
          title: "Conversations",
          tabBarIcon: conversationsIcon,
        }}
      />
      <Tabs.Screen
        name="routines"
        options={{
          title: "Routines",
          tabBarIcon: routinesIcon,
        }}
      />
      <Tabs.Screen
        name="settings"
        options={{
          title: "Settings",
          tabBarIcon: settingsIcon,
        }}
      />
    </Tabs>
  );
}

function conversationsIcon({ color }: { color: ColorValue }) {
  return <Icon name="conversations" color={color} />;
}

function routinesIcon({ color }: { color: ColorValue }) {
  return <Icon name="routines" color={color} />;
}

function settingsIcon({ color }: { color: ColorValue }) {
  return <Icon name="settings" color={color} />;
}
