import type { Snapshot } from "@swarmie/client-ui";
import type { ComponentType, ReactNode } from "react";

import type { Message } from "./types";

export type Dispose = () => void;
export type ActionContext = {
  conversationId: string;
  nodeId: string;
  signal: AbortSignal;
  insertText: (text: string) => void;
};
export type ExtensionIcon = ComponentType<{ className?: string }>;
export type ConversationControlProps = { conversationId: string; isDraft: boolean };
export type ComposerTopBarItem = {
  id: string;
  owner: string;
  side: "left" | "right";
  priority: number;
  component: ComponentType<ConversationControlProps>;
};
export type ConversationSummary = {
  id: string;
  title: string;
  pinned: boolean;
  archived: boolean;
  started: boolean;
};
export type ConversationGroupProps = {
  conversations: readonly ConversationSummary[];
  activeId: string;
  openConversation: (id: string) => void;
  renderConversations: (items: readonly ConversationSummary[]) => ReactNode;
};
export type ConversationGroup = {
  id: string;
  owner: string;
  order?: number;
  component: ComponentType<ConversationGroupProps>;
  membership: Snapshot<readonly string[]>;
};
export type ConversationHeaderItem = {
  id: string;
  owner: string;
  order?: number;
  component: ComponentType<ConversationControlProps>;
};
export type ComposerAction = {
  id: string;
  owner: string;
  label: string;
  icon: ExtensionIcon;
  slot: "leading" | "before-send";
  order?: number;
  enabled?: (context: ActionContext) => boolean;
  execute: (context: ActionContext) => void | Promise<void>;
};
export type SlashCommand = Omit<ComposerAction, "slot"> & { command: string; description?: string };
export type AttachmentAction = Omit<ComposerAction, "slot"> & { section: string };
export type MessageAction = Omit<ComposerAction, "slot" | "enabled" | "execute"> & {
  roles?: readonly Message["role"][];
  execute: (context: ActionContext & { message: Message }) => void | Promise<void>;
};
export type MessageRenderer = {
  id: string;
  owner: string;
  order?: number;
  matches: (message: Message) => boolean;
  component: ComponentType<{ message: Message }>;
};
export type ConversationsUiApi = {
  composer: {
    topBar: {
      items: Snapshot<readonly ComposerTopBarItem[]>;
      registerItem: (item: ComposerTopBarItem) => Dispose;
    };
    actions: Snapshot<readonly ComposerAction[]>;
    slashCommands: Snapshot<readonly SlashCommand[]>;
    attachmentActions: Snapshot<readonly AttachmentAction[]>;
    registerAction: (action: ComposerAction) => Dispose;
    registerSlashCommand: (command: SlashCommand) => Dispose;
    registerAttachmentAction: (action: AttachmentAction) => Dispose;
  };
  navigation: {
    groups: Snapshot<readonly ConversationGroup[]>;
    registerGroup: (group: ConversationGroup) => Dispose;
  };
  header: {
    items: Snapshot<readonly ConversationHeaderItem[]>;
    registerItem: (item: ConversationHeaderItem) => Dispose;
  };
  messages: {
    actions: Snapshot<readonly MessageAction[]>;
    renderers: Snapshot<readonly MessageRenderer[]>;
    registerAction: (action: MessageAction) => Dispose;
    registerRenderer: (renderer: MessageRenderer) => Dispose;
  };
  lifecycle: {
    registerCreated: (observer: {
      id: string;
      owner: string;
      execute: (conversationId: string) => void;
    }) => Dispose;
    created: (conversationId: string) => void;
  };
  dispose: Dispose;
};

/** These executable local contributions live only in the owner's opaque Tembra resource. */
export function createConversationsUi(): ConversationsUiApi {
  let disposed = false;
  function slot<T extends { id: string; owner: string; order?: number }>() {
    let items: readonly T[] = [];
    const listeners = new Set<() => void>();
    const publish = () => {
      for (const listener of listeners) {
        listener();
      }
    };
    return {
      getSnapshot: () => items,
      subscribe: (listener: () => void) => {
        if (disposed) {
          return () => undefined;
        }
        listeners.add(listener);
        return () => {
          listeners.delete(listener);
        };
      },
      register: (item: T) => {
        if (disposed) {
          throw new Error("Conversations is no longer active.");
        }
        if (items.some((existing) => existing.id === item.id)) {
          throw new Error(`Duplicate contribution: ${item.id}`);
        }
        items = [...items, item].toSorted(
          (a, b) => (a.order ?? 0) - (b.order ?? 0) || a.id.localeCompare(b.id),
        );
        publish();
        return () => {
          if (!items.includes(item)) {
            return;
          }
          items = items.filter((existing) => existing !== item);
          publish();
        };
      },
      clear() {
        items = [];
        publish();
        listeners.clear();
      },
      refresh() {
        items = [...items];
        publish();
      },
    };
  }
  const created = slot<{ id: string; owner: string; execute: (conversationId: string) => void }>();
  const actions = slot<ComposerAction>();
  const topBar = slot<ComposerTopBarItem>();
  const groups = slot<ConversationGroup>();
  const headers = slot<ConversationHeaderItem>();
  const groupDisposers = new Set<Dispose>();
  const commands = slot<SlashCommand>();
  const attachments = slot<AttachmentAction>();
  const messageActions = slot<MessageAction>();
  const renderers = slot<MessageRenderer>();
  return {
    composer: {
      topBar: { items: topBar, registerItem: topBar.register },
      actions,
      slashCommands: commands,
      attachmentActions: attachments,
      registerAction: actions.register,
      registerSlashCommand: commands.register,
      registerAttachmentAction: attachments.register,
    },
    navigation: {
      groups,
      registerGroup(group) {
        const remove = groups.register(group);
        const detach = group.membership.subscribe(() => {
          groups.refresh();
        });
        const dispose = () => {
          detach();
          remove();
          groupDisposers.delete(dispose);
        };
        groupDisposers.add(dispose);
        return dispose;
      },
    },
    header: { items: headers, registerItem: headers.register },
    lifecycle: {
      registerCreated: created.register,
      created(conversationId) {
        for (const observer of created.getSnapshot()) {
          try {
            observer.execute(conversationId);
          } catch (error: unknown) {
            console.error("Conversation creation observer failed", error);
          }
        }
      },
    },
    messages: {
      actions: messageActions,
      renderers,
      registerAction: messageActions.register,
      registerRenderer: renderers.register,
    },
    dispose() {
      disposed = true;
      for (const dispose of groupDisposers) {
        dispose();
      }
      for (const collection of [
        actions,
        created,
        commands,
        attachments,
        messageActions,
        renderers,
        topBar,
        groups,
        headers,
      ]) {
        collection.clear();
      }
    },
  };
}
