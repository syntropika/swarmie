import { useSource } from "@swarmie/client-ui";
import type { Conversation } from "@swarmie/plugin-conversations-browser";
import { Button, LoadingStatus } from "@swarmie/ui-web";

import { useClient } from "../../client/context";

export function ComposerFeedback({
  chat,
  showStatus,
}: {
  chat: Conversation;
  showStatus: () => void;
}) {
  const { connectionState, submissionIssue } = useClient();
  const draft = useSource(chat.draft);
  const execution = useSource(chat.execution);
  const connections = useSource(connectionState);
  const status = connections.sessions[chat.node ?? draft.node]?.phase ?? "disconnected";
  const connecting = status === "connecting" || status === "reconnecting";
  const issue = submissionIssue(chat);
  if (!connecting && issue === null && execution.phase !== "unknown") {
    return null;
  }
  return (
    <div className="mb-2 grid min-w-0 justify-items-start gap-2 px-2">
      {connecting ? <LoadingStatus>Connecting to your node…</LoadingStatus> : null}
      {issue === null ? null : (
        <p role="alert" className="text-caption text-danger">
          {issue}
        </p>
      )}
      {execution.phase === "unknown" ? (
        <Button variant="ghost" size="sm" onPress={showStatus}>
          View request status
        </Button>
      ) : null}
    </div>
  );
}
