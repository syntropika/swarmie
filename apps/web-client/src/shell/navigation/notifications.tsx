import { useSource } from "@swarmie/client-ui";

import { useClient } from "../../client/context";

export function NotificationsNavigation({ onClose }: { onClose: () => void }) {
  const { notifications, openConversation } = useClient();
  const updates = useSource(notifications);
  return (
    <>
      <div className="mb-3 px-1 text-chrome text-muted">Notifications</div>
      {updates.length ? (
        updates.map((update) => (
          <button
            key={update.id}
            type="button"
            onClick={() => {
              if (update.conversationId !== undefined) {
                openConversation(update.conversationId);
              }
              onClose();
            }}
            className="mb-1 w-full rounded-md p-2 text-left hover:bg-row-hover"
          >
            <div className="truncate text-xs">{update.title}</div>
            <div className="mt-1 text-chrome text-muted">{update.text}</div>
          </button>
        ))
      ) : (
        <p className="px-1 text-xs leading-6 text-muted">Conversation updates will appear here.</p>
      )}
    </>
  );
}
