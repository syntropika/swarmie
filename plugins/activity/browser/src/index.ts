import { updatePresentation } from "@swarmie/client-ui";
import type { Snapshot, UiStore } from "@swarmie/client-ui";
import { observeNodeService } from "@swarmie/plugin-browser/connections";
import type { Connections, ConnectionsState } from "@swarmie/plugin-browser/connections";
import { orb } from "@syntropika/tembra-sdk";

export type ActivityEntry = {
  id: string;
  owner: string;
  entity: string;
  title: string;
  text: string;
  conversationId?: string;
  unread: boolean;
  updatedAt: number;
};
export type ActivityInput = Omit<ActivityEntry, "id" | "updatedAt" | "unread">;
export type ActivityState = {
  items: ActivityEntry[];
  revision: number;
  available: boolean;
  nodeId: string | null;
  principal: string | null;
  error: string | null;
};
export const activityOrb = orb({
  name: "swarmie.activity.ui",
  apiVersion: 1,
  services: {
    "swarmie.activity.ui": {
      create: (self) =>
        self.state<ActivityState>({
          items: [],
          revision: 0,
          available: false,
          nodeId: null,
          principal: null,
          error: null,
        }),
      methods: { update: updatePresentation<ActivityState> },
    },
  },
});

function object(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}
function activityEntry(value: unknown): value is ActivityEntry {
  return (
    object(value) &&
    typeof value.id === "string" &&
    typeof value.owner === "string" &&
    typeof value.entity === "string" &&
    typeof value.title === "string" &&
    typeof value.text === "string" &&
    (value.conversationId === undefined || typeof value.conversationId === "string") &&
    typeof value.unread === "boolean" &&
    typeof value.updatedAt === "number" &&
    Number.isSafeInteger(value.updatedAt) &&
    value.updatedAt >= 0 &&
    value.id === JSON.stringify([value.owner, value.entity])
  );
}
export function decodeActivity(value: unknown) {
  if (
    !object(value) ||
    !Array.isArray(value.items) ||
    !value.items.every(activityEntry) ||
    typeof value.revision !== "number" ||
    !Number.isSafeInteger(value.revision) ||
    value.revision < 0
  ) {
    throw new Error("The node returned invalid activity entries.");
  }
  return { items: value.items, revision: value.revision };
}

/** The native inbox owns publication and read state. Browser owns only its projection. */
export function createActivityApi(
  state: UiStore<Readonly<ActivityState>>,
  nodes: Snapshot<Readonly<ConnectionsState>>,
  connections: Connections,
) {
  let generation = 0;
  const binding = observeNodeService(nodes, connections, "swarmie.activity", {
    reset(nodeId, principal) {
      generation += 1;
      state.set({ items: [], revision: 0, available: false, nodeId, principal, error: null });
    },
    receive(value) {
      const snapshot = decodeActivity(value);
      state.update((current) =>
        snapshot.revision < current.revision ? current : { ...current, ...snapshot },
      );
    },
    failed(error) {
      state.update((current) => ({
        ...current,
        available: false,
        error: error instanceof Error ? error.message : String(error),
      }));
    },
    ready() {
      state.update((current) => ({ ...current, available: true, error: null }));
    },
  });
  async function change(input: unknown) {
    const admitted = generation;
    const receipt = await binding.invoke("change", input);
    if (admitted !== generation) {
      return;
    }
    const minimum =
      receipt !== null &&
      typeof receipt === "object" &&
      "revision" in receipt &&
      typeof receipt.revision === "number"
        ? receipt.revision
        : 0;
    try {
      await binding.read(minimum);
    } catch (error: unknown) {
      state.update((current) =>
        admitted === generation
          ? {
              ...current,
              error: error instanceof Error ? error.message : String(error),
            }
          : current,
      );
    }
  }
  return {
    state,
    async publish(input: ActivityInput) {
      await change({ kind: "publish", input });
    },
    async read(id: string) {
      await change({ kind: "read", id });
    },
    close: () => {
      binding.close();
    },
  };
}
