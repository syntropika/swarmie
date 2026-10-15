import { createFileRoute, redirect } from "@tanstack/react-router";

import { HomePage } from "~/src/marketing/components/home-page";
import { getPreferredRequestLocale } from "~/src/platform/i18n/request-locale";

export const Route = createFileRoute("/")({
  beforeLoad: async ({ location }) => {
    const locale = await getPreferredRequestLocale();
    if (locale !== "en") {
      throw redirect({
        to: "/$locale/",
        params: { locale },
        search: location.search,
        hash: location.hash,
        statusCode: 302,
        headers: {
          Vary: "Origin, Accept-Language",
          "Cache-Control": "private, no-store",
        },
      });
    }
  },
  component: HomePage,
});
