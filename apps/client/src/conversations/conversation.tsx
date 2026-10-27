import { useChat } from "../controls";
import { Composer } from "./composer/composer";
import { Transcript } from "./transcript";

export function ConversationPanel() {
  const chat = useChat();
  return (
    <section
      aria-label="Conversation"
      className="flex h-full min-h-0 min-w-0 flex-col bg-background"
    >
      <Transcript key={`transcript-${chat.id}`} chat={chat} />
      <Composer key={`composer-${chat.id}`} chat={chat} />
    </section>
  );
}
