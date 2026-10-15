import { HeadContent, Link, Scripts, createRootRoute } from "@tanstack/react-router";
import type { ReactNode } from "react";

import {
  alternateLocaleLinks,
  canonicalLocaleUrl,
  localeFromPath,
} from "~/src/platform/i18n/locale";
import { LocaleProvider, useLocale } from "~/src/platform/i18n/provider";
import { isProductionSiteRequest } from "~/src/platform/i18n/request-locale";
import { createTranslator } from "~/src/platform/i18n/translate";

import stylesheet from "~/src/styles.css?url";

export const Route = createRootRoute({
  loader: async ({ location }) => {
    const locale = localeFromPath(location.pathname);
    return {
      locale: locale ?? "en",
      indexed: locale !== undefined && (await isProductionSiteRequest()),
    };
  },
  head: ({ loaderData }) => {
    const locale = loaderData?.locale ?? "en";
    const t = createTranslator(locale);
    const indexed = loaderData?.indexed === true;
    const canonical = indexed ? canonicalLocaleUrl(locale) : undefined;
    return {
      meta: [
        { charSet: "utf-8" },
        { name: "viewport", content: "width=device-width, initial-scale=1" },
        { title: t("meta.title") },
        { name: "description", content: t("meta.description") },
        { name: "theme-color", content: "#F4F1EC" },
        { property: "og:title", content: t("meta.title") },
        { property: "og:description", content: t("meta.socialDescription") },
        { property: "og:type", content: "website" },
        ...(canonical === undefined ? [] : [{ property: "og:url", content: canonical }]),
        ...(indexed ? [] : [{ name: "robots", content: "noindex, nofollow" }]),
      ],
      links: [
        { rel: "stylesheet", href: stylesheet },
        { rel: "icon", type: "image/svg+xml", href: "/mark.svg" },
        ...(canonical === undefined
          ? []
          : [{ rel: "canonical", href: canonical }, ...alternateLocaleLinks()]),
      ],
    };
  },
  shellComponent: Root,
  notFoundComponent: NotFound,
});

function Root({ children }: { children: ReactNode }) {
  const locale = Route.useLoaderData()?.locale ?? "en";
  return (
    <html
      lang={locale}
      data-theme="light"
      className="scroll-pt-6 scrollbar-thumb-drawing-line scrollbar-track-canvas scroll-smooth motion-reduce:scroll-auto"
    >
      <head>
        <HeadContent />
      </head>
      <body className="bg-canvas font-sans leading-relaxed text-ink antialiased selection:bg-primary-soft selection:text-ink [&_:focus-visible]:outline-2 [&_:focus-visible]:outline-offset-2 [&_:focus-visible]:outline-primary [&_a]:underline-offset-4 [&_a:focus-visible]:rounded-pill [&_a:focus-visible]:outline-offset-4 [&_h1]:text-balance [&_h2]:text-balance [&_h3]:text-balance [&_summary:focus-visible]:outline-offset-4">
        <LocaleProvider locale={locale}>{children}</LocaleProvider>
        <Scripts />
      </body>
    </html>
  );
}

function NotFound() {
  const { t } = useLocale();
  return (
    <div className="mx-auto flex min-h-svh w-[min(1248px,calc(100%_-_48px))] flex-col md:w-[min(1248px,calc(100%_-_96px))]">
      <main className="flex flex-1 flex-col items-start justify-center gap-6">
        <h1 className="text-title font-medium">{t("common.notFoundTitle")}</h1>
        <p className="text-text-muted">{t("common.notFound")}</p>
        <Link to="/" className="text-primary underline underline-offset-4">
          {t("common.backHome")}
        </Link>
      </main>
    </div>
  );
}
