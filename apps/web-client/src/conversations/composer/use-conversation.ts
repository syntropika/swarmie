import { useSource } from "@swarmie/client-ui";

import { useClient } from "../../client/context";

export function useComposerConversation(id: string) {
  const { roster } = useClient();
  const chats = useSource(roster);
  const chat = chats.find((entry) => entry.id === id);
  if (chat === undefined) {
    throw new Error("The composer conversation is unavailable.");
  }
  return chat;
}
