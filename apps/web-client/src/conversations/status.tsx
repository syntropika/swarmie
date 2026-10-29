import { useSource } from "@swarmie/client-ui";
import type { Conversation, Execution } from "@swarmie/plugin-conversations-browser";
import { Button } from "@swarmie/ui-web";
import { ThinkingIndicator } from "@swarmie/ui-web/elements/conversation";
import { useState } from "react";

import { useClient } from "../client/context";

export function activityLabel(execution: Execution): string {
  const labels: Record<Execution["phase"], string> = {
    idle: "",
    completed: "",
    queued: "Waiting to start…",
    working: "Working…",
    stopping: "Stopping…",
    stopped: "Stopped",
    failed: "The request failed. You can send a new message.",
    unknown: "The request outcome is not confirmed.",
  };
  if (["stopped", "failed", "unknown"].includes(execution.phase) && execution.label !== "") {
    return execution.label;
  }
  return labels[execution.phase];
}

export function ConversationStatus({ chat }: { chat: Conversation }) {
  const { acknowledgeUnknown, conversationState, conversationError } = useClient();
  const execution = useSource(chat.execution);
  const presentationError = useSource(conversationError);
  const [acknowledging, setAcknowledging] = useState(false);
  const label = activityLabel(execution);
  const busy = ["queued", "working", "stopping"].includes(execution.phase);
  async function acknowledge() {
    setAcknowledging(true);
    try {
      await acknowledgeUnknown(chat);
    } catch (error: unknown) {
      conversationState.update((current) => ({
        ...current,
        error: error instanceof Error ? error.message : String(error),
      }));
    } finally {
      setAcknowledging(false);
    }
  }
  return (
    <section aria-label="Request status" className="grid min-w-0 justify-items-start gap-2 pb-6">
      {busy ? <ThinkingIndicator label={label} /> : null}
      {!busy && label !== "" ? (
        <output
          className={`text-caption ${execution.phase === "failed" ? "text-danger" : "text-muted"}`}
        >
          {label}
        </output>
      ) : null}
      {execution.phase === "unknown" && execution.canAcknowledge === true ? (
        <>
          <p className="text-caption text-muted">
            A tool may have changed external data. This action closes the unfinished records and
            does not retry the tool.
          </p>
          <Button
            variant="secondary"
            size="sm"
            isPending={acknowledging}
            isDisabled={acknowledging}
            onPress={() => {
              acknowledge().catch(() => undefined);
            }}
          >
            Acknowledge unknown outcome
          </Button>
        </>
      ) : null}
      {presentationError === null ? null : (
        <p role="alert" className="text-caption text-danger">
          {presentationError}
        </p>
      )}
    </section>
  );
}
