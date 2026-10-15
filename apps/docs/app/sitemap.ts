import type { MetadataRoute } from "next";

import { source } from "@/lib/source";

export const revalidate = false;
export default function sitemap(): MetadataRoute.Sitemap {
  const origin = process.env.DOCS_SITE_URL;
  if (origin === undefined || origin === "") {
    return [];
  }
  return [
    { url: new URL("/", origin).href },
    ...source.getPages().map((page) => ({ url: new URL(page.url, origin).href })),
  ];
}
