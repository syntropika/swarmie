import { DocsLayout } from "fumadocs-ui/layouts/docs";

import { baseOptions } from "~/src/lib/layout.shared";
import { source } from "~/src/lib/source";

export default function Layout({ children }: LayoutProps<"/docs">) {
  return (
    <DocsLayout tree={source.getPageTree()} {...baseOptions()}>
      {children}
    </DocsLayout>
  );
}
