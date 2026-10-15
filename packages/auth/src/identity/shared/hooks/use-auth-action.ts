import { useMutation } from "@tanstack/react-query";
import { useEffect, useRef } from "react";

import { AuthPasskeyError } from "~/src/identity/authentication/failures";
import type { AuthFailure } from "~/src/identity/authentication/failures";

type AuthActionOptions<Result> = {
  run: (signal: AbortSignal) => Promise<Result>;
  onError?: (error: AuthFailure) => void;
};

/** Query owns request state; this synchronous guard prevents duplicate one-use proof submissions. */
export function useAuthAction<Result>({ run, onError }: AuthActionOptions<Result>) {
  const operation = useRef<AbortController | null>(null);
  const mutation = useMutation<Result, AuthFailure>({
    mutationFn: async () => {
      const active = operation.current;
      if (!active) {
        throw new AuthPasskeyError({ code: "cancelled", message: "The request was cancelled." });
      }
      return await run(active.signal);
    },
    retry: false,
    networkMode: "always",
    gcTime: 0,
    onError: (error) => {
      if (operation.current && !operation.current.signal.aborted) {
        onError?.(error);
      }
    },
    onSettled: () => {
      operation.current = null;
    },
  });

  useEffect(() => () => operation.current?.abort(), []);

  return {
    ...mutation,
    execute: () => {
      if (operation.current) {
        return;
      }
      operation.current = new AbortController();
      mutation.mutate();
    },
  };
}
