import { createServerFn } from "@tanstack/react-start";
import { getRequestHeader, getRequestUrl, setResponseHeader } from "@tanstack/react-start/server";

import { preferredLocaleFromAcceptLanguage } from "~/src/platform/i18n/locale";

/** Preview hosts must not advertise production canonicals or enter search indexes. */
export const isProductionSiteRequest = createServerFn({ method: "GET" }).handler(
  () => getRequestUrl().origin === "https://swarmie.ai",
);

/** Use browser preference only when a visitor opens the unprefixed landing. */
export const getPreferredRequestLocale = createServerFn({ method: "GET" }).handler(() => {
  setResponseHeader("Vary", "Origin, Accept-Language");
  setResponseHeader("Cache-Control", "private, no-store");
  return preferredLocaleFromAcceptLanguage(getRequestHeader("accept-language"));
});
