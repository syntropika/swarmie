import { errorMessage, selectStore } from "@swarmie/client-ui";
import type { Snapshot, UiStore } from "@swarmie/client-ui";

import type { ConversationData, ConversationsState, Draft } from "./types";

export const defaultConversationTitle = "New conversation";

/** Live fields share the owning conversation State. */
export type Conversation = Omit<ConversationData, "draft" | "messages" | "execution"> & {
  draft: UiStore<Draft>;
  messages: UiStore<ConversationData["messages"]>;
  execution: UiStore<ConversationData["execution"]>;
};

const changeRecord =
  (id: string, change: (record: ConversationData) => ConversationData) =>
  (current: Readonly<ConversationsState>): ConversationsState => ({
    ...current,
    records: current.records.map((record) => (record.id === id ? change(record) : record)),
  });

/** Keep record views stable while their owner publishes changes. */
export function createConversationRecords(
  state: UiStore<Readonly<ConversationsState>>,
  commit: (
    change: (current: Readonly<ConversationsState>) => ConversationsState,
  ) => Promise<unknown>,
) {
  const facades = new Map<string, Conversation>();
  let previousRecords: readonly ConversationData[] = [];
  let previousRoster: Conversation[] = [];

  function facade(data: ConversationData): Conversation {
    const existing = facades.get(data.id);
    if (existing) {
      return existing;
    }
    const record = selectStore(
      state,
      (value) => value.records.find((entry) => entry.id === data.id) ?? data,
      (value, next) => changeRecord(data.id, () => next)(value),
    );
    const view: Conversation = {
      get id() {
        return data.id;
      },
      get title() {
        return record.getSnapshot().title;
      },
      get node() {
        return record.getSnapshot().node;
      },
      get started() {
        return record.getSnapshot().started;
      },
      get executionContext() {
        return record.getSnapshot().executionContext;
      },
      get pinned() {
        return record.getSnapshot().pinned;
      },
      get archived() {
        return record.getSnapshot().archived;
      },
      draft: selectStore(
        record,
        (value) => value.draft,
        (value, draft) => ({ ...value, draft }),
      ),
      messages: selectStore(
        record,
        (value) => value.messages,
        (value, messages) => ({ ...value, messages }),
      ),
      execution: selectStore(
        record,
        (value) => value.execution,
        (value, execution) => ({ ...value, execution }),
      ),
    };
    facades.set(data.id, view);
    return view;
  }

  const roster: Snapshot<readonly Conversation[]> = {
    subscribe: state.subscribe,
    getSnapshot() {
      const { records } = state.getSnapshot();
      if (records === previousRecords) {
        return previousRoster;
      }
      const same =
        previousRecords.length === records.length &&
        records.every((record, index) => {
          const previous = previousRecords[index];
          return (
            previous?.id === record.id &&
            previous.title === record.title &&
            previous.started === record.started &&
            previous.node === record.node &&
            previous.executionContext === record.executionContext &&
            previous.pinned === record.pinned &&
            previous.archived === record.archived
          );
        });
      if (!same) {
        previousRoster = records.map(facade);
      }
      previousRecords = records;
      return previousRoster;
    },
  };

  return {
    roster,
    create: (data: ConversationData): Conversation => {
      const view = facade(data);
      state.update((current) => ({ ...current, records: [...current.records, data] }));
      return view;
    },
    update: (id: string, change: (record: ConversationData) => ConversationData): void => {
      state.update(changeRecord(id, change));
    },
    commit: async (
      id: string,
      change: (record: ConversationData) => ConversationData,
    ): Promise<unknown> => await commit(changeRecord(id, change)),
    fail: (error: unknown): void => {
      state.update((current) => ({ ...current, error: errorMessage(error) }));
    },
  };
}
