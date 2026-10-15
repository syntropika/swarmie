import { resolveLinkItems } from "fumadocs-ui/layouts/shared";
import type { BaseLayoutProps } from "fumadocs-ui/layouts/shared";

import { SwarmieLogo } from "@/components/brand";

import { gitConfig } from "./shared";

export function baseOptions(): BaseLayoutProps {
  const links = resolveLinkItems({
    links: [{ text: "Documentation", url: "/docs", active: "nested-url" }],
    githubUrl: `https://github.com/${gitConfig.user}/${gitConfig.repo}`,
  }).map((link) =>
    "icon" in link ? { ...link, icon: <span aria-hidden="true">{link.icon}</span> } : link,
  );

  return {
    nav: {
      title: <SwarmieLogo />,
    },
    links,
  };
}
