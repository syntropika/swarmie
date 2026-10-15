import { useMutation } from "@tanstack/react-query";
import { useRef } from "react";

import { submitSubscription, SubscriptionError } from "~/src/newsletter/client/subscribe";
import type { PlainMessageKey } from "~/src/platform/i18n/catalogs/en";

const errorMessages: Record<SubscriptionError["reason"], PlainMessageKey> = {
  "invalid-email": "newsletter.invalidEmail",
  "rate-limit": "newsletter.rateLimit",
  unavailable: "newsletter.unavailable",
  network: "newsletter.network",
  "invalid-response": "newsletter.invalidResponse",
};

/** Keep submitted addresses out of Query variables and release them when the request settles. */
export function useNewsletter() {
  const submission = useRef<{ email: string; website: string } | null>(null);
  const mutation = useMutation<undefined, SubscriptionError>({
    mutationFn: async (): Promise<undefined> => {
      const fields = submission.current;
      if (!fields) {
        throw new SubscriptionError({ reason: "invalid-response" });
      }
      await submitSubscription(fields.email, fields.website);
    },
    retry: false,
    networkMode: "always",
    gcTime: 0,
    onSettled: () => {
      submission.current = null;
    },
  });

  return {
    pending: mutation.isPending,
    complete: mutation.isSuccess,
    error: mutation.error ? errorMessages[mutation.error.reason] : undefined,
    subscribe: (email: string, website: string) => {
      if (submission.current) {
        return;
      }
      submission.current = { email, website };
      mutation.mutate();
    },
  };
}
