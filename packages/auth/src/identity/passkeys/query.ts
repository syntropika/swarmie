import { queryOptions } from "@tanstack/react-query";

import { authQueryKeys } from "~/src/identity/authentication/cache";
import type { AuthEffects } from "~/src/identity/authentication/effects";
import { runAuthEffect } from "~/src/identity/authentication/effects";

/** Retain account-scoped labels across navigation, while refreshing them whenever Account opens. */
export function passkeysQueryOptions(userId: string, authEffects: AuthEffects) {
  return queryOptions({
    queryKey: authQueryKeys.passkeys(userId),
    queryFn: async ({ signal }) => await runAuthEffect(authEffects.passkeys(), { signal }),
    staleTime: 0,
    gcTime: 5 * 60_000,
  });
}
