import { useSource } from "@swarmie/client-ui";
import { ContextPanel } from "@swarmie/plugin-context-browser/views";

import { useClient } from "../client/context";

export function PinnedContext() {
  const { shell, context } = useClient();
  const layout = useSource(shell);
  if (!layout.summary || layout.section !== "conversations") {
    return null;
  }
  return (
    <aside
      aria-label="Summary"
      className="absolute top-2 right-2 z-30 max-h-[calc(100%-16px)] w-72 max-w-[calc(100%-16px)] overflow-y-auto rounded-panel bg-overlay shadow-overlay"
    >
      <ContextPanel api={context} />
    </aside>
  );
}
