import { Surface } from "@swarmie/ui-web";
import type { ReactNode } from "react";

import { PageFooter } from "~/src/platform/shell/components/page-footer";

/** Focused account and authentication content shares a centered page footer. */
export function MessagePage({ children }: { children: ReactNode }) {
  return (
    <div className="mx-auto flex min-h-svh w-full max-w-[640px] flex-col px-6 py-6 sm:py-10">
      <main className="flex min-w-0 flex-1 flex-col justify-center py-6">
        <Surface elevation="raised">
          <div className="flex flex-col items-start gap-6">{children}</div>
        </Surface>
      </main>
      <PageFooter />
    </div>
  );
}
