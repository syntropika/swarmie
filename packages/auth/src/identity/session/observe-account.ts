import type { User } from "@swarmie/api";
import type { QueryClient } from "@tanstack/react-query";

import { authQueryKeys } from "~/src/identity/authentication/cache";
import { clearAccountQueries } from "~/src/platform/account-cache";
import { observeBackgroundTask } from "~/src/platform/background-task";

/** An admitted view cannot keep former-account resources after the shared identity changes. */
export function observeSessionAccount(
  client: QueryClient,
  admittedUserId: string,
  onChanged: () => Promise<unknown>,
): () => void {
  let active = true;
  let clearing = false;
  const check = () => {
    const user = client.getQueryData<User>(authQueryKeys.session);
    if (!active || clearing || !user || user.id === admittedUserId) {
      return;
    }
    clearing = true;
    observeBackgroundTask(
      clearAccountQueries(client).then(async () => {
        if (active) {
          await onChanged();
        }
      }),
    );
  };
  const unsubscribe = client.getQueryCache().subscribe(check);
  check();
  return () => {
    active = false;
    unsubscribe();
  };
}
