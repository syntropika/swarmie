import type { Conversation } from "@swarmie/plugin-conversations-browser";
import { Paperclip, X } from "lucide-react";

import { useSelection } from "../../client/use-selection";
import { IconButton, iconStyle } from "../../controls";
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
          className="flex max-w-full min-w-0 items-center gap-1 rounded-small bg-default px-2 py-1 text-chrome"
        >
          <Paperclip className={iconStyle} />
          <span className="min-w-0 truncate" title={name}>
            {name}
          </span>
          <IconButton
            icon={X}
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
