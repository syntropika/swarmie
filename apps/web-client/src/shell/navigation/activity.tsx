import { useSource } from "@swarmie/client-ui";
import { ChromeButton } from "@swarmie/ui-web";

import { useClient } from "../../client/context";

export function ActivityNavigation({ onClose }: { onClose: () => void }) {
  const { activity, activityApi, openConversation } = useClient();
  const updates = useSource(activity);
  const inbox = useSource(activityApi.state);
  return (
    <>
      <div className="mb-3 px-1 text-navigation-heading text-muted">Activity</div>
      {inbox.error === null ? null : (
        <p role="alert" className="px-1 text-caption text-danger">
          {inbox.error}
        </p>
      )}
      {updates.length ? (
        updates.map((update) => (
          <ChromeButton
            key={update.id}
            size="navigation"
            onPress={() => {
              activityApi.read(update.id).catch(reportReadFailure);
              if (update.conversationId !== undefined) {
                openConversation(update.conversationId);
              }
              onClose();
            }}
            className="mb-1 flex-col items-start"
          >
            <span className="max-w-full truncate">
              {update.unread ? "• " : ""}
              {update.title}
            </span>
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

function reportReadFailure(error: unknown) {
  console.error("The activity entry could not be marked as read.", error);
}
