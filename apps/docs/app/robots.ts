import type { MetadataRoute } from 'next';

export const revalidate = false;
export default function robots(): MetadataRoute.Robots {
  const origin = process.env.DOCS_SITE_URL;
  return { rules: { userAgent: '*', allow: '/' }, sitemap: origin ? new URL('/sitemap.xml', origin).href : undefined };
}
