import { useSource } from "@swarmie/client-ui";
import type { Conversation } from "@swarmie/plugin-conversations-browser";
import { ChromeButton } from "@swarmie/ui-web";
import { ArrowUp, Square } from "lucide-react";
import { useSyncExternalStore } from "react";

import { useClient } from "../../client/context";
import { iconStyle } from "../../controls";
import type { DeliveryMode } from "./state";

export function ComposerSubmit({ chat, mode }: { chat: Conversation; mode: DeliveryMode }) {
  const { canSend, subscribeExecution, send, stop, connectionState, providers } = useClient();
  const draft = useSource(chat.draft);
  const execution = useSource(chat.execution);
  useSource(connectionState);
  useSource(providers.state);
  const ready = useSyncExternalStore(
    subscribeExecution,
    () => canSend(chat),
    () => false,
  );
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
        <Square className={`${iconStyle} fill-current`} />
      </ChromeButton>
    );
  }
  return (
    <ChromeButton
      size="composerSend"
      aria-label={busy ? `Send to ${mode}` : "Send message"}
      isDisabled={!ready}
      onPress={() => {
        send(chat, mode);
      }}
    >
      <ArrowUp className={iconStyle} />
    </ChromeButton>
  );
}
