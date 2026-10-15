import { createFileRoute, notFound } from "@tanstack/react-router";

import { HomePage } from "~/src/marketing/components/home-page";
import { isLocale } from "~/src/platform/i18n/locale";

export const Route = createFileRoute("/$locale/")({
  beforeLoad: ({ params }) => {
    if (!isLocale(params.locale) || params.locale === "en") {
      notFound({ throw: true });
    }
  },
  component: HomePage,
});
