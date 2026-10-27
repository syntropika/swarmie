import type { Conversation } from "@swarmie/plugin-conversations-browser";

import { Item, Menu } from "../../controls";
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
        <>
          {(["queue", "steer"] as const).map((entry) => (
            <Item
              key={entry}
              onClick={() => {
                setMode(entry);
                close();
              }}
            >
              {entry === "queue" ? "Queue for the next turn" : "Steer at the next step"}
            </Item>
          ))}
        </>
      )}
    </Menu>
  );
}
