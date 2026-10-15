import type { Metadata } from "next";
import type { ReactNode } from "react";

import { Provider } from "~/components/provider";

import "~/app/global.css";

export const metadata: Metadata = {
  metadataBase:
    process.env.DOCS_SITE_URL !== undefined && process.env.DOCS_SITE_URL !== ""
      ? new URL(process.env.DOCS_SITE_URL)
      : undefined,
  title: { default: "Swarmie documentation", template: "%s | Swarmie" },
  description:
    "Use Swarmie, connect execution nodes, integrate coding agents, and build extensions with the public runtime and client contracts.",
  icons: {
    icon: [
      {
        url: "/brand/swarmie-mark-mono-ink.svg",
        type: "image/svg+xml",
        media: "(prefers-color-scheme: light)",
      },
      {
        url: "/brand/swarmie-mark-mono-white.svg",
        type: "image/svg+xml",
        media: "(prefers-color-scheme: dark)",
      },
    ],
  },
};

export default function Layout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" suppressHydrationWarning>
      <body className="flex min-h-screen flex-col">
        <Provider>{children}</Provider>
      </body>
    </html>
  );
}
