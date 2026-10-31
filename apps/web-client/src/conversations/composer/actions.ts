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
  const { conversationId, controller: scope } = useMemo(
    () => ({ conversationId: chat.id, controller: new AbortController() }),
    [chat.id],
  );
  useEffect(
    () => () => {
      scope.abort();
    },
    [scope],
  );

  const actionContext = (): ActionContext => ({
    conversationId,
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
    if (scope.signal.aborted) {
      return;
    }
    conversationState.update((current) => ({
      ...current,
      error: error instanceof Error ? error.message : String(error),
    }));
  }

  async function run(action: Pick<ComposerAction, "execute">) {
    await action.execute(actionContext());
  }

  async function executeConfirmed(action: Pick<ComposerAction, "execute">) {
    try {
      await run(action);
      return true;
    } catch (error: unknown) {
      failed(error);
      return false;
    }
  }
  function executeAction(action: Pick<ComposerAction, "execute">) {
    executeConfirmed(action).catch(failed);
  }
  return { actionContext, executeAction, executeConfirmed };
}

export type ComposerActions = ReturnType<typeof useComposerActions>;

/** Consume only the command draft that received a confirmed result. A newer edit
 * is checked inside its owning State update, including queued local publications. */
export async function executeSlashCommand(
  chat: Conversation,
  text: string,
  action: Pick<ComposerAction, "execute">,
  execute: ComposerActions["executeConfirmed"],
) {
  if (await execute(action)) {
    chat.draft.update((current) => (current.text === text ? { ...current, text: "" } : current));
  }
}
