import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";

import { useAuthentication, useNavigate } from "~/src/host";
import { isUnauthorized, runAuthEffect } from "~/src/identity/authentication/effects";
import { getPasskeySupport } from "~/src/identity/passkeys/browser";
import type { PasskeyManagementProps } from "~/src/identity/passkeys/components/passkey-management";
import { passkeyChangeError, passkeyChangeFeedback } from "~/src/identity/passkeys/feedback";
import type { PasskeyChange, PasskeyChangeResult } from "~/src/identity/passkeys/feedback";
import { passkeysQueryOptions } from "~/src/identity/passkeys/query";
import type { LoginFeedback } from "~/src/identity/shared/feedback";
import { clearAccountQueries } from "~/src/platform/account-cache";
import { observeBackgroundTask } from "~/src/platform/background-task";

/** Query owns the saved list and mutation lifecycle; only the editable label stays in local state. */
export function usePasskeyAccount(userId: string): PasskeyManagementProps {
  const { effects: authEffects } = useAuthentication();

  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const options = passkeysQueryOptions(userId, authEffects);
  const keys = useQuery(options);
  const [name, setName] = useState("");
  const operation = useRef<AbortController | null>(null);
  const change = useMutation({
    mutationFn: async (action: PasskeyChange): Promise<PasskeyChangeResult> => {
      const controller = operation.current;
      if (!controller) {
        throw new Error("Passkey operation was cancelled");
      }
      if (action === "register") {
        await runAuthEffect(authEffects.registerPasskey(name, controller.signal), {
          signal: controller.signal,
        });
        setName("");
      } else {
        await runAuthEffect(authEffects.revokePasskey(action.revokeId), {
          signal: controller.signal,
        });
        queryClient.setQueryData(options.queryKey, (current) =>
          current?.filter((key) => key.id !== action.revokeId),
        );
      }
      try {
        await queryClient.query(options);
        return { action, refreshed: true };
      } catch (error) {
        if (isUnauthorized(error)) {
          await clearAccountQueries(queryClient);
          await navigate({ to: "/login", replace: true });
        }
        return { action, refreshed: false };
      }
    },
    onSettled: () => {
      operation.current = null;
    },
  });

  useEffect(() => () => operation.current?.abort(), []);
  useEffect(() => {
    if (isUnauthorized(keys.error) || isUnauthorized(change.error)) {
      observeBackgroundTask(
        clearAccountQueries(queryClient).then(async () => {
          await navigate({ to: "/login", replace: true });
        }),
      );
    }
  }, [keys.error, change.error, queryClient, navigate]);

  function feedback(): LoginFeedback | undefined {
    if (change.isPending) {
      return undefined;
    }
    if (change.isError) {
      return passkeyChangeError(change.error);
    }
    if (change.data) {
      return passkeyChangeFeedback(change.data);
    }
    if (keys.isError) {
      return {
        tone: "error",
        title: "keys.loadFailedTitle",
        message: "keys.loadFailed",
      };
    }
    return undefined;
  }

  function submit(action: PasskeyChange) {
    if (operation.current) {
      return;
    }
    operation.current = new AbortController();
    change.mutate(action);
  }

  return {
    passkeys: (keys.data ?? []).map((key) => ({
      id: key.id,
      name: key.name,
      createdAt: key.created_at,
      lastUsedAt: key.last_used_at,
    })),
    name,
    loading: keys.isPending,
    pending: change.isPending ? (change.variables ?? null) : null,
    available: getPasskeySupport() === "available",
    feedback: feedback(),
    onNameChange: (value) => {
      setName(value);
      if (!change.isPending) {
        change.reset();
      }
    },
    onRegister: () => {
      submit("register");
    },
    onRevoke: (id) => {
      submit({ revokeId: id });
    },
    onCancel: () => operation.current?.abort(),
  };
}
