import { queryOptions } from "@tanstack/react-query";

import { authQueryKeys } from "~/src/identity/authentication/cache";
import type { AuthEffects } from "~/src/identity/authentication/effects";
import { runAuthEffect } from "~/src/identity/authentication/effects";

/** Retain browser identity across private views; refresh stale data without delaying navigation. */
export function createSessionQueryOptions(authEffects: AuthEffects) {
  return queryOptions({
    queryKey: authQueryKeys.session,
    queryFn: async ({ signal }) => await runAuthEffect(authEffects.session(), { signal }),
    networkMode: "always",
    refetchOnWindowFocus: true,
    refetchOnReconnect: true,
    staleTime: 10_000,
    gcTime: 5 * 60_000,
  });
}
