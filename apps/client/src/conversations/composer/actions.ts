import type {
  ActionContext,
  ComposerAction,
  Conversation,
} from "@swarmie/plugin-conversations-browser";
import { useEffect, useMemo } from "react";

import { useClient } from "../../client/context";

/** Keep plugin actions within this Composer's lifetime. */
export function useComposerActions(chat: Conversation) {
  const { conversationState, conversationNode } = useClient();
  const scope = useMemo(() => new AbortController(), []);
  useEffect(
    () => () => {
      scope.abort();
    },
    [scope],
  );

  const actionContext = (): ActionContext => ({
    conversationId: chat.id,
    nodeId: conversationNode(chat),
    signal: scope.signal,
    insertText: (text) => {
      if (!scope.signal.aborted) {
        chat.draft.update((current) => ({
          ...current,
          text: current.text ? `${current.text}\n${text}` : text,
        }));
      }
    },
  });

  function failed(error: unknown) {
    conversationState.update((current) => ({
      ...current,
      error: error instanceof Error ? error.message : String(error),
    }));
  }

  async function run(action: Pick<ComposerAction, "execute">) {
    await action.execute(actionContext());
  }

  function executeAction(action: Pick<ComposerAction, "execute">) {
    run(action).catch(failed);
  }

  return { actionContext, executeAction };
}

export type ComposerActions = ReturnType<typeof useComposerActions>;
