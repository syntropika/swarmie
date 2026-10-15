export const locales = ["en", "es", "fr", "de", "pt", "it", "ja", "zh-CN"] as const;
export type Locale = (typeof locales)[number];

const publicSiteOrigin = "https://swarmie.ai";

export function isLocale(value: unknown): value is Locale {
  return typeof value === "string" && locales.some((locale) => locale === value);
}

/** Choose the highest-priority supported browser language, preserving header order for ties. */
export function preferredLocaleFromAcceptLanguage(header: string | undefined): Locale {
  let preferred: Locale = "en";
  let highestQuality = -1;

  for (const range of header?.slice(0, 2048).split(",").slice(0, 24) ?? []) {
    const [rawTag, ...parameters] = range.split(";");
    const tag = rawTag?.trim().toLowerCase();
    if (tag === undefined || tag === "" || !/^[a-z]{1,8}(?:-[a-z0-9]{1,8})*$/u.test(tag)) {
      continue;
    }

    const qualityParameter = parameters.find((parameter) => parameter.trim().startsWith("q="));
    const qualityText = qualityParameter?.trim().slice(2);
    const quality = qualityText === undefined ? 1 : Number(qualityText);
    if (qualityText === "" || !Number.isFinite(quality) || quality <= 0 || quality > 1) {
      continue;
    }

    const [primary] = tag.split("-");
    let locale: Locale | undefined;
    if (primary === "zh") {
      if (tag === "zh" || /^zh-(?:cn|sg|hans)(?:-|$)/u.test(tag)) {
        locale = "zh-CN";
      }
    } else if (isLocale(primary)) {
      locale = primary;
    }
    if (locale && quality > highestQuality) {
      preferred = locale;
      highestQuality = quality;
    }
  }

  return preferred;
}

/** Each translated landing has one stable, crawlable path. */
export function localePath(locale: Locale): string {
  return locale === "en" ? "/" : `/${locale}/`;
}

/** Match complete landing paths so unknown paths cannot inherit a translated canonical URL. */
export function localeFromPath(pathname: string): Locale | undefined {
  if (pathname === "/") {
    return "en";
  }
  const segments = pathname.split("/");
  if (segments.length !== 2 && segments.length !== 3) {
    return undefined;
  }
  if (segments.length === 3 && segments[2] !== "") {
    return undefined;
  }
  return isLocale(segments[1]) && segments[1] !== "en" ? segments[1] : undefined;
}

export function canonicalLocaleUrl(locale: Locale): string {
  return `${publicSiteOrigin}${localePath(locale)}`;
}

export function alternateLocaleLinks(): {
  rel: "alternate";
  hrefLang: Locale | "x-default";
  href: string;
}[] {
  return [
    ...locales.map((locale) => ({
      rel: "alternate" as const,
      hrefLang: locale,
      href: canonicalLocaleUrl(locale),
    })),
    { rel: "alternate", hrefLang: "x-default", href: canonicalLocaleUrl("en") },
  ];
}
