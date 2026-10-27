import type { UiStore } from "@swarmie/client-ui";

import type { Tab, ViewsState } from "./index";

type Conversation = { id: string; title: string };

/** Tab commands update Views without changing application navigation. */
export function createViewsApi(state: UiStore<Readonly<ViewsState>>, nextId: () => string) {
  return {
    open: (kind: Tab["kind"], conversation: Conversation): void => {
      state.update((current) => {
        const existing =
          kind === "start" ? undefined : current.tabs.find((tab) => tab.kind === kind);
        const tab: Tab = existing ?? {
          id: nextId(),
          kind,
          title: {
            start: "New tab",
            notes: "Project notes",
            state: "App state",
            conversation: conversation.title,
          }[kind],
        };
        return {
          ...current,
          tabs: existing === undefined ? [...current.tabs, tab] : current.tabs,
          tab: tab.id,
          mobile: "view",
        };
      });
    },
    close: (id: string): void => {
      state.update((current) => {
        const removed = current.tabs.find((tab) => tab.id === id);
        const tabs = current.tabs.filter((tab) => tab.id !== id);
        const restoreConversation = removed?.kind === "conversation" || tabs.length === 0;
        return {
          ...current,
          tabs,
          tab: current.tab === id ? (tabs.at(-1)?.id ?? "") : current.tab,
          conversationVisible: restoreConversation ? true : current.conversationVisible,
          expanded: restoreConversation ? false : current.expanded,
          mobile: tabs.length === 0 ? "conversation" : current.mobile,
        };
      });
    },
    expand: (conversation: Conversation): void => {
      state.update((current) => {
        if (current.expanded) {
          return {
            ...current,
            tabs: current.tabs.filter((tab) => tab.kind !== "conversation"),
            tab: current.tabs.find((tab) => tab.kind !== "conversation")?.id ?? "",
            expanded: false,
            conversationVisible: true,
          };
        }
        const tab: Tab = {
          id: nextId(),
          kind: "conversation",
          title: conversation.title,
          conversationId: conversation.id,
        };
        return {
          ...current,
          tabs: [...current.tabs.filter((entry) => entry.kind !== "conversation"), tab],
          expanded: true,
          conversationVisible: false,
        };
      });
    },
  };
}
