// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import { useMutation } from "@tanstack/react-query";

import { errorMessage, errorReporter, startAction } from "~/src/action-boundary";

/** Account mutations have no automatic retry: a receipt is replayed only on a user's request. */
export function useAccountAction(onNotice: (message: string) => void) {
  const mutation = useMutation({
    mutationFn: async (action: () => Promise<unknown>) => await action(),
    retry: false,
    gcTime: 0,
  });
  const run = (action: () => Promise<unknown>) => {
    onNotice("");
    startAction(mutation.mutateAsync(action), errorReporter(onNotice));
  };
  return {
    run,
    busy: mutation.isPending,
    error: mutation.error ? errorMessage(mutation.error) : "",
  };
}
