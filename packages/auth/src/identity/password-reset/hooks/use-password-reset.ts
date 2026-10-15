import { useEffect, useState } from "react";

import { useAuthentication } from "~/src/host";
import { runAuthEffect } from "~/src/identity/authentication/effects";
import { authError } from "~/src/identity/authentication/errors";
import { useAuthAction } from "~/src/identity/shared/hooks/use-auth-action";

/** The reset token stays in memory; the address is scrubbed before the user submits. */
export function usePasswordReset() {
  const { effects: authEffects } = useAuthentication();

  // oxlint-disable-next-line react/hook-use-state -- The one-use proof is an immutable snapshot retained after the URL is scrubbed.
  const [token] = useState(() => new URL(window.location.href).searchParams.get("token") ?? "");
  const [password, setPassword] = useState("");
  const action = useAuthAction({
    run: async (signal) => {
      try {
        await runAuthEffect(authEffects.resetPassword(token, password), { signal });
      } finally {
        setPassword("");
      }
    },
  });

  useEffect(() => {
    window.history.replaceState(window.history.state, "", window.location.pathname);
  }, []);

  function stage(): "incomplete" | "complete" | "form" {
    if (!token) {
      return "incomplete";
    }
    if (action.isSuccess) {
      return "complete";
    }
    return "form";
  }

  return {
    stage: stage(),
    password,
    pending: action.isPending,
    feedback: action.error ? authError(action.error, "reset-password") : undefined,
    changePassword: (value: string) => {
      setPassword(value);
      if (!action.isPending) {
        action.reset();
      }
    },
    submit: () => {
      if (token) {
        action.execute();
      }
    },
  };
}
