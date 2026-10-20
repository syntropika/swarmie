import { HeroUINativeProviderRaw } from "heroui-native/provider-raw";
import type { ReactNode } from "react";
import { useReducedMotion } from "react-native-reanimated";

/** Keep vendor setup inside the native UI package; clients own safe-area and gesture roots. */
export function NativeProvider({ children }: { children: ReactNode }) {
  const reducedMotion = useReducedMotion();
  return (
    <HeroUINativeProviderRaw config={reducedMotion ? { animation: "disable-all" } : {}}>
      {children}
    </HeroUINativeProviderRaw>
  );
}
