import type { UiStore } from "@swarmie/client-ui";

import type { Tab, ViewsState } from "./index";

type Conversation = { id: string; title: string };

function isTemporaryConversation(tab: Tab): boolean {
  return tab.kind === "conversation" && tab.temporary === true;
}

/** Returns the conversation from its temporary tab to the dedicated panel. */
function restoreConversation(current: Readonly<ViewsState>): ViewsState {
  const tabs = current.tabs.filter((tab) => !isTemporaryConversation(tab));
  return {
    ...current,
    tabs,
    tab: tabs.some((tab) => tab.id === current.tab) ? current.tab : (tabs.at(-1)?.id ?? ""),
    expanded: false,
    conversationVisible: true,
  };
}

/** Tab commands update Views without changing application navigation. */
export function createViewsApi(state: UiStore<Readonly<ViewsState>>, nextId: () => string) {
  return {
    showConversation: (): void => {
      state.update((current) => ({ ...restoreConversation(current), mobile: "conversation" }));
    },
    toggleConversation: (): void => {
      state.update((current) =>
        current.expanded
          ? restoreConversation(current)
          : {
              ...current,
              conversationVisible: current.tabs.length === 0 || !current.conversationVisible,
            },
      );
    },
    open: (kind: Tab["kind"], conversation: Conversation): void => {
      state.update((current) => {
        const existing = current.tabs.find((tab) =>
          kind === "conversation"
            ? tab.kind === "conversation" &&
              !tab.temporary &&
              tab.conversationId === conversation.id
            : kind !== "start" && tab.kind === kind,
        );
        const tab: Tab =
          existing ??
          (kind === "conversation"
            ? { id: nextId(), kind, title: conversation.title, conversationId: conversation.id }
            : {
                id: nextId(),
                kind,
                title: {
                  start: "New tab",
                  notes: "Project notes",
                  state: "App state",
                }[kind],
              });
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
        const restoringConversation =
          (removed !== undefined && isTemporaryConversation(removed)) || tabs.length === 0;
        return {
          ...current,
          tabs,
          tab: current.tab === id ? (tabs.at(-1)?.id ?? "") : current.tab,
          conversationVisible: restoringConversation ? true : current.conversationVisible,
          expanded: restoringConversation ? false : current.expanded,
          mobile: tabs.length === 0 ? "conversation" : current.mobile,
        };
      });
    },
    expand: (conversation: Conversation): void => {
      state.update((current) => {
        if (current.expanded) {
          return restoreConversation(current);
        }
        const tab: Tab = {
          id: nextId(),
          kind: "conversation",
          title: conversation.title,
          conversationId: conversation.id,
          temporary: true,
        };
        return {
          ...current,
          tabs: [...current.tabs.filter((entry) => !isTemporaryConversation(entry)), tab],
          expanded: true,
          conversationVisible: false,
        };
      });
    },
  };
}
