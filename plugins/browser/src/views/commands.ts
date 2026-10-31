import type { UiStore } from "@swarmie/client-ui";

import type { BuiltinTab, CapabilityView, Tab, ViewsState, ViewValue } from "./index";

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

function valueKey(value: ViewValue): string {
  if (value === null || typeof value !== "object") {
    return JSON.stringify(value);
  }
  if (Array.isArray(value)) {
    return `[${value.map(valueKey).join(",")}]`;
  }
  return `{${Object.entries<ViewValue>(value)
    .toSorted(([left], [right]) => left.localeCompare(right))
    .map(([key, item]) => `${JSON.stringify(key)}:${valueKey(item)}`)
    .join(",")}}`;
}

// Property order is not identity: capability owners supply a stable explicit target.
const sameTarget = (left: CapabilityView["target"], right: CapabilityView["target"]) =>
  valueKey(left) === valueKey(right);

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
    open: (kind: BuiltinTab["kind"], conversation: Conversation): void => {
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
    openCapability: (view: CapabilityView & { title: string }): void => {
      state.update((current) => {
        const existing = current.tabs.find(
          (tab) =>
            tab.kind === "capability" &&
            tab.owner === view.owner &&
            tab.view === view.view &&
            sameTarget(tab.target, view.target),
        );
        const starting = current.tabs.find((tab) => tab.id === current.tab && tab.kind === "start");
        const remaining = current.tabs.filter((tab) => tab.id !== starting?.id);
        const tab: Tab = existing ?? {
          ...view,
          id: starting?.id ?? nextId(),
          kind: "capability",
          target: structuredClone(view.target),
        };
        return {
          ...current,
          tabs: existing ? remaining : [...remaining, tab],
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
        if (current.tabs.some((tab) => tab.id === current.tab && tab.kind === "capability")) {
          return { ...current, expanded: true, conversationVisible: false };
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
          tab: current.tab || tab.id,
          expanded: true,
          conversationVisible: false,
        };
      });
    },
  };
}
