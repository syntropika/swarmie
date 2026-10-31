import { useSource } from "@swarmie/client-ui";
import { Button } from "@swarmie/ui-web";

import { useClient } from "../client/context";

export function VaultPager({ section }: { section: string }) {
  const { vault } = useClient();
  const state = useSource(vault.state);
  const page = state.pages[section] ?? state.lifecycle.pages[section];
  if (!page || (page.previous === null && page.next === null)) {
    return null;
  }
  return (
    <nav aria-label={`${section} pages`} className="mt-3 flex flex-wrap items-center gap-2">
      <span className="text-xs text-muted">
        {section} · {page.offset + 1} of {page.total}
      </span>
      <Button
        variant="tertiary"
        isDisabled={page.previous === null}
        onPress={() => {
          if (page.previous !== null) {
            void vault.page(section, page.previous).catch(() => false);
          }
        }}
      >
        Previous
      </Button>
      <Button
        variant="tertiary"
        isDisabled={page.next === null}
        onPress={() => {
          if (page.next !== null) {
            void vault.page(section, page.next).catch(() => false);
          }
        }}
      >
        Next
      </Button>
    </nav>
  );
}
