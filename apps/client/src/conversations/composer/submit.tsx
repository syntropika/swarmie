import { useSource } from "@swarmie/client-ui";
import type { Conversation } from "@swarmie/plugin-conversations-browser";
import { ChromeButton } from "@swarmie/ui-web";
import { ArrowUp, Square } from "lucide-react";

import { useClient } from "../../client/context";

export function ComposerSubmit({ chat, mode }: { chat: Conversation; mode: "queue" | "steer" }) {
  const { canSend, send, stop } = useClient();
  const draft = useSource(chat.draft);
  const execution = useSource(chat.execution);
  const busy = ["queued", "working", "stopping"].includes(execution.phase);
  if (busy && draft.text.trim() === "") {
    return (
      <ChromeButton
        size="composerSend"
        aria-label="Stop response"
        isDisabled={execution.phase === "stopping"}
        onPress={() => {
          stop(chat);
        }}
      >
        <Square className="size-3 fill-current" />
      </ChromeButton>
    );
  }
  return (
    <ChromeButton
      size="composerSend"
      aria-label={busy ? `Send to ${mode}` : "Send message"}
      isDisabled={!canSend(chat)}
      onPress={() => {
        send(chat, mode);
      }}
    >
      <ArrowUp className="size-4" />
    </ChromeButton>
  );
}
