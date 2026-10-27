import type { Conversation } from "@swarmie/plugin-conversations-browser";

import { ChoiceList, Menu } from "../../controls";
import { useComposerBusy } from "./state";
import type { DeliveryMode } from "./state";

export function ComposerDelivery({
  chat,
  mode,
  setMode,
}: {
  chat: Conversation;
  mode: DeliveryMode;
  setMode: (mode: DeliveryMode) => void;
}) {
  const busy = useComposerBusy(chat);
  if (!busy) {
    return null;
  }
  return (
    <Menu
      label="Message delivery"
      text={mode === "queue" ? "Queue" : "Steer"}
      size="composerCap"
      placement="top end"
    >
      {(close) => (
        <ChoiceList
          label="Message delivery"
          value={mode}
          items={(["queue", "steer"] as const).map((entry) => ({
            id: entry,
            label: entry === "queue" ? "Queue for the next turn" : "Steer at the next step",
          }))}
          onAction={(entry) => {
            setMode(entry);
            close();
          }}
        />
      )}
    </Menu>
  );
}
