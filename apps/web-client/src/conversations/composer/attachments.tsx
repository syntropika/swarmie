import type { Conversation } from "@swarmie/plugin-conversations-browser";
import { Paperclip, Plus } from "lucide-react";

import { useSelection } from "../../client/use-selection";
import { IconButton } from "../../controls";
import { draftAttachments } from "./state";

export function ComposerAttachments({ chat }: { chat: Conversation }) {
  const attachments = useSelection(chat.draft, draftAttachments);
  if (!attachments.length) {
    return null;
  }
  return (
    <div className="flex flex-wrap gap-1.5 px-3 pt-2">
      {attachments.map((name) => (
        <span
          key={name}
          className="flex items-center gap-1 rounded-md bg-default px-2 py-1 text-chrome"
        >
          <Paperclip className="size-3" />
          {name}
          <IconButton
            icon={Plus}
            size="tabClose"
            label={`Remove ${name}`}
            onPress={() => {
              chat.draft.update((draft) => ({
                ...draft,
                attachments: draft.attachments.filter((entry) => entry !== name),
              }));
            }}
          />
        </span>
      ))}
    </div>
  );
}
