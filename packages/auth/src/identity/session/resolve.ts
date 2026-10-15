import type { User } from "@swarmie/api";
import type { QueryClient } from "@tanstack/react-query";

import { authQueryKeys } from "~/src/identity/authentication/cache";

/** Reuse current account identity only while its query remains successful and valid. */
export function readCachedSession(client: QueryClient): User | undefined {
  const state = client.getQueryState<User>(authQueryKeys.session);
  return state?.status === "success" && state.error === null && !state.isInvalidated
    ? state.data
    : undefined;
}
