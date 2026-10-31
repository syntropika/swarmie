import type { Snapshot } from "@swarmie/client-ui";
import type { Selection } from "@swarmie/plugin-context-browser";
import type { Conversation } from "@swarmie/plugin-conversations-browser";

/** Observe the active conversation's actual node choice without copying its draft. */
export function contextSelection(
  active: () => Conversation,
  sources: Snapshot<unknown>[],
): Snapshot<Selection | null> {
  let cached: Selection | null = null;
  function read() {
    const chat = active();
    const nodeId = chat.node ?? chat.draft.getSnapshot().node;
    if (cached?.nodeId !== nodeId || cached?.conversationId !== chat.id) {
      cached = nodeId ? { nodeId, conversationId: chat.id } : null;
    }
    return cached;
  }
  return {
    getSnapshot: read,
    subscribe(changed) {
      let chat = active();
      let detachDraft = chat.draft.subscribe(changed);
      const refresh = () => {
        const next = active();
        if (chat !== next) {
          detachDraft();
          chat = next;
          detachDraft = chat.draft.subscribe(changed);
        }
        changed();
      };
      const detach = sources.map((source) => source.subscribe(refresh));
      return () => {
        detachDraft();
        for (const remove of detach) {
          remove();
        }
      };
    },
  };
}
