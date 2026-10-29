import { NativeButton, NativeField, NativeNotice } from "@swarmie/ui-native";
import type { ComponentProps, ReactNode } from "react";
import { SafeAreaView } from "react-native-safe-area-context";
import { withUniwind } from "uniwind";

export const SafeView = withUniwind(SafeAreaView);

export function Action({
  label,
  onPress,
  disabled = false,
  selected = false,
}: {
  label: string;
  onPress: () => void;
  disabled?: boolean;
  selected?: boolean;
}) {
  return (
    <NativeButton
      label={label}
      onPress={onPress}
      disabled={disabled}
      tone={selected ? "primary" : "secondary"}
      accessibilityState={{ selected }}
    />
  );
}

export function Field({ label, ...props }: ComponentProps<typeof NativeField>) {
  return <NativeField label={label} {...props} />;
}

export function Notice({ children }: { children: ReactNode }) {
  return <NativeNotice>{children}</NativeNotice>;
}

export function Page({ children }: { children: ReactNode }) {
  return (
    <SafeView edges={["top", "left", "right", "bottom"]} className="flex-1 bg-native-canvas">
      {children}
    </SafeView>
  );
}
