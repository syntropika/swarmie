import { Archive, MoreHorizontal, Pencil, Pin, PinOff } from "lucide-react";

import { Button } from "~/src/components/ui/button";
import { Dropdown } from "~/src/components/ui/dropdown";
import { Tooltip, TooltipContent } from "~/src/components/ui/tooltip";
import { ContributionSlot } from "~/src/contribution-host";
import type { Thread } from "~/src/protocol";

export function SidebarChatActions({
  thread,
  onPin,
  onRename,
  onArchive,
}: {
  thread: Thread;
  onPin: () => void;
  onRename: () => void;
  onArchive: () => void;
}) {
  const pinLabel = thread.pinned ? "Unpin chat" : "Pin chat";
  const archiveLabel = thread.archived === true ? "Restore chat" : "Archive chat";
  const busy = ["starting", "running", "needs_input", "cancelling"].includes(thread.state);
  return (
    <div className="absolute inset-y-0 right-1 flex items-center gap-0.5">
      <div
        className={
          thread.pinned
            ? ""
            : "pointer-events-none opacity-0 group-focus-within/chat-row:pointer-events-auto group-focus-within/chat-row:opacity-100 group-hover/chat-row:pointer-events-auto group-hover/chat-row:opacity-100"
        }
      >
        <Tooltip>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={`${pinLabel}: ${thread.title}`}
            onClick={onPin}
            tone="muted"
          >
            {thread.pinned ? <PinOff size={14} /> : <Pin size={14} />}
          </Button>
          <TooltipContent>{pinLabel}</TooltipContent>
        </Tooltip>
      </div>
      <div className="pointer-events-none opacity-0 group-focus-within/chat-row:pointer-events-auto group-focus-within/chat-row:opacity-100 group-hover/chat-row:pointer-events-auto group-hover/chat-row:opacity-100">
        <Dropdown triggerTooltip={<TooltipContent>Chat options</TooltipContent>}>
          <Button
            variant="ghost"
            size="icon-sm"
            tone="muted"
            aria-label={`Chat options: ${thread.title}`}
          >
            <MoreHorizontal size={14} />
          </Button>
          <Dropdown.Popover placement="bottom end">
            <Dropdown.Menu aria-label={`Actions for ${thread.title}`}>
              <ContributionSlot
                name="conversation.contextMenu"
                context={{
                  conversationId: thread.id,
                  projectId: thread.projectId || undefined,
                  worktreeId: thread.worktreeId || undefined,
                  sessionId: thread.sessionId,
                  composerReady: false,
                }}
                menu
              />
              <Dropdown.Item id="rename" onAction={onRename}>
                <Pencil size={14} />
                Rename chat
              </Dropdown.Item>
              <Dropdown.Item id="pin" onAction={onPin}>
                {thread.pinned ? <PinOff size={14} /> : <Pin size={14} />}
                {pinLabel}
              </Dropdown.Item>
              <Dropdown.Item id="archive" isDisabled={busy} onAction={onArchive}>
                <Archive size={14} />
                {archiveLabel}
              </Dropdown.Item>
            </Dropdown.Menu>
          </Dropdown.Popover>
        </Dropdown>
      </div>
    </div>
  );
}
