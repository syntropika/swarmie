import { requireOptionalNativeModule } from "expo-modules-core";

import { MobileClient, NativeClientError } from "./client";
import type { NativeBinding } from "./client";

export { MobileClient, NativeClientError } from "./client";
export type { NativeBinding } from "./client";

export function connectNativeClient(config: { url: string; token: string }): MobileClient {
  const binding = requireOptionalNativeModule<NativeBinding>("SwarmieClient");
  if (!binding) {
    throw new NativeClientError("native_build_required");
  }
  return new MobileClient(binding, config);
}
