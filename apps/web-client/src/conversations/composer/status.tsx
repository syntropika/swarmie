import { useSource } from "@swarmie/client-ui";
import type { Conversation } from "@swarmie/plugin-conversations-browser";

import { useClient } from "../../client/context";
import { useSelection } from "../../client/use-selection";
import { draftNode, executionLabel } from "./state";

export function ComposerStatus({ chat }: { chat: Conversation }) {
  const { connectionState } = useClient();
  const node = useSelection(chat.draft, draftNode);
  const connections = useSource(connectionState);
  const status = connections.sessions[chat.node ?? node]?.phase ?? "disconnected";
  const label = useSelection(chat.execution, executionLabel);
  return (
    <>
      {status === "connecting" || status === "reconnecting" ? (
        <output className="mt-2 flex items-center justify-center gap-2 text-xs text-muted">
          <span className="size-2 animate-pulse rounded-full border border-brand-accent" />
          Connecting to your node…
        </output>
      ) : null}
      {label ? <output className="mt-2 text-center text-xs text-muted">{label}</output> : null}{" "}
    </>
  );
}
