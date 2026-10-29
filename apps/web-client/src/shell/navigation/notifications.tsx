import { useSource } from "@swarmie/client-ui";
import { ChromeButton } from "@swarmie/ui-web";

import { useClient } from "../../client/context";

export function NotificationsNavigation({ onClose }: { onClose: () => void }) {
  const { notifications, openConversation } = useClient();
  const updates = useSource(notifications);
  return (
    <>
      <div className="mb-3 px-1 text-navigation-heading text-muted">Notifications</div>
      {updates.length ? (
        updates.map((update) => (
          <ChromeButton
            key={update.id}
            size="navigation"
            onPress={() => {
              if (update.conversationId !== undefined) {
                openConversation(update.conversationId);
              }
              onClose();
            }}
            className="mb-1 flex-col items-start"
          >
            <span className="max-w-full truncate">{update.title}</span>
            <span className="text-muted">{update.text}</span>
          </ChromeButton>
        ))
      ) : (
        <p className="px-1 text-caption leading-6 text-muted">
          Conversation updates will appear here.
        </p>
      )}
    </>
  );
}
