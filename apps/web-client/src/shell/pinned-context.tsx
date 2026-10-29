import { useSource } from "@swarmie/client-ui";

import { useClient } from "../client/context";

export function PinnedContext() {
  const { shell } = useClient();
  const layout = useSource(shell);
  if (!layout.summary || layout.section !== "conversations") {
    return null;
  }
  return (
    <aside
      aria-label="Summary"
      className="absolute top-2 right-2 z-30 h-48 w-72 max-w-[calc(100vw-24px)] rounded-panel bg-overlay shadow-overlay"
    />
  );
}
