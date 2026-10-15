import type { User } from "@swarmie/api";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";

import { useSessionQueryOptions, useNavigate } from "~/src/host";
import { useSessionExpiry } from "~/src/identity/session/hooks/use-session-expiry";
import { observeSessionAccount } from "~/src/identity/session/observe-account";

/** All admitted private views share refresh, expiry, and account-replacement behavior. */
export function useSessionObserver(user: User, ...errors: unknown[]) {
  const sessionQueryOptions = useSessionQueryOptions();
  const client = useQueryClient();
  const navigate = useNavigate();
  const session = useQuery(sessionQueryOptions);
  const expired = useSessionExpiry(session.error, ...errors);
  const changed = session.data !== undefined && session.data.id !== user.id;
  useEffect(
    () =>
      observeSessionAccount(client, user.id, async () => {
        await navigate({ to: "/", replace: true });
      }),
    [client, navigate, user.id],
  );
  return { session, expired, changed, hidden: expired || changed || session.isPending };
}
