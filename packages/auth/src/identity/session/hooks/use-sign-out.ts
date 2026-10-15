import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useRef } from "react";

import { useAuthentication, useNavigate } from "~/src/host";
import { runAuthEffect } from "~/src/identity/authentication/effects";
import { clearAccountQueries } from "~/src/platform/account-cache";

/** Account settings and avatar actions share the same explicit, single-submission logout. */
export function useSignOut() {
  const { effects: authEffects } = useAuthentication();

  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const submitting = useRef(false);
  const mutation = useMutation({
    mutationFn: async () => {
      await runAuthEffect(authEffects.logout());
    },
    retry: false,
    onSuccess: async () => {
      await clearAccountQueries(queryClient);
      await navigate({ to: "/login", replace: true });
    },
    onSettled: () => {
      submitting.current = false;
    },
  });

  return {
    error: mutation.error,
    failed: mutation.isError,
    pending: mutation.isPending,
    reset: mutation.reset,
    signOut: () => {
      if (submitting.current) {
        return;
      }
      submitting.current = true;
      mutation.mutate();
    },
  };
}
