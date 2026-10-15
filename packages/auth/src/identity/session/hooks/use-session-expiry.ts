import { isUnauthorized } from "@swarmie/api";
import { useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";

import { useNavigate } from "~/src/host";
import { clearAccountQueries } from "~/src/platform/account-cache";
import { observeBackgroundTask } from "~/src/platform/background-task";

/** Hide private content immediately while an expired session is cleared and redirected. */
export function useSessionExpiry(...errors: unknown[]): boolean {
  const expired = errors.some(isUnauthorized);
  const client = useQueryClient();
  const navigate = useNavigate();

  useEffect(() => {
    if (!expired) {
      return undefined;
    }
    let active = true;
    observeBackgroundTask(
      clearAccountQueries(client).then(() => {
        if (active) {
          observeBackgroundTask(navigate({ to: "/login", replace: true }));
        }
      }),
    );
    return () => {
      active = false;
    };
  }, [expired, client, navigate]);

  return expired;
}
