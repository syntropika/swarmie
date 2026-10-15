import type { User } from "@swarmie/api";

import { useSessionObserver } from "~/src/identity/session/hooks/use-session-observer";
import { useSignOut } from "~/src/identity/session/hooks/use-sign-out";
import { observeBackgroundTask } from "~/src/platform/background-task";

/** Session queries own request state; logout remains an explicit operation with one recovery read. */
export function useAccountSession(user: User) {
  const logout = useSignOut();

  const { session, expired, hidden } = useSessionObserver(user, logout.error);

  return {
    expired,
    hidden,
    user: session.data,
    error: session.isError || logout.failed,
    pending: logout.pending,
    retry: () => {
      logout.reset();
      observeBackgroundTask(session.refetch());
    },
    logout: logout.signOut,
  };
}
