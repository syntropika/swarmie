import { createFileRoute } from "@tanstack/react-router";
import { env } from "cloudflare:workers";

import { createResendNewsletter } from "~/src/newsletter/server/resend.server";
import { handleSubscription } from "~/src/newsletter/server/subscribe";

export const Route = createFileRoute("/api/subscribe")({
  server: {
    handlers: {
      POST: async ({ request }) => {
        const apiKey = env.RESEND_NEWSLETTER_API_KEY?.trim();
        const subscriptions = apiKey
          ? createResendNewsletter({
              apiKey,
              segmentId: env.RESEND_NEWSLETTER_SEGMENT_ID?.trim(),
            })
          : undefined;
        return await handleSubscription(request, subscriptions);
      },
    },
  },
});
