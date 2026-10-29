import type { Snapshot, UiStore } from "@swarmie/client-ui";
import { updatePresentation } from "@swarmie/client-ui";
import type {
  Connections,
  ConnectionsState,
  Node,
  NodeConnectionState,
} from "@swarmie/plugin-connections-browser";
import { ConnectionPhase } from "@swarmie/plugin-connections-browser";
import { orb } from "@syntropika/tembra-sdk";

import { errorMessage } from "./checkout-data";
import { GitRemote, gitRequestKinds } from "./remote";

export type GitNodeAvailability = {
  principal: string;
  generation: number;
  available: boolean;
  error: string | null;
};
export type GitBrowserState = { enabled: boolean; nodes: Record<string, GitNodeAvailability> };

export const gitBrowserOrb = orb({
  name: "swarmie.git.ui",
  apiVersion: 1,
  services: {
    "swarmie.git.ui": {
      create: (self) => self.state<GitBrowserState>({ enabled: false, nodes: {} }),
      methods: { update: updatePresentation<GitBrowserState> },
    },
  },
});

/** A complete observed interface is availability evidence, never an access grant. */
export function createGitPresentation(
  state: UiStore<Readonly<GitBrowserState>>,
  connectionsState: Snapshot<Readonly<ConnectionsState>>,
  connections: Pick<Connections, "open">,
) {
  let active = false;
  let lifetime = 0;
  let probes = new Map<string, string>();
  let refreshVersion = 0;
  async function probe(node: Node, session: NodeConnectionState, key: string, epoch: number) {
    let failure: string | null = null;
    try {
      const connection = await connections.open(node.id);
      if (connection.principal !== node.principal || connection.generation !== session.generation) {
        throw new Error("The Git node identity changed during capability discovery.");
      }
      const result = await new GitRemote(connection).request({ kind: "capability_info" });
      if (
        result.kind !== "capability_info" ||
        result.api_version !== 2 ||
        !result.execution_available ||
        gitRequestKinds.some((kind) => !result.operations.includes(kind))
      ) {
        throw new Error(
          "The node does not provide the complete Git interface and execution support required by this view.",
        );
      }
    } catch (error: unknown) {
      failure = errorMessage(error);
    }
    const latest = connectionsState.getSnapshot();
    if (
      !active ||
      epoch !== lifetime ||
      probes.get(node.id) !== key ||
      latest.nodes.find((entry) => entry.id === node.id)?.principal !== node.principal ||
      latest.sessions[node.id]?.phase !== ConnectionPhase.Connected ||
      latest.sessions[node.id]?.generation !== session.generation
    ) {
      return;
    }
    state.update((value) => ({
      ...value,
      nodes: {
        ...value.nodes,
        [node.id]: {
          principal: node.principal,
          generation: session.generation,
          available: failure === null,
          error: failure,
        },
      },
    }));
  }
  async function observe() {
    if (!active) {
      return;
    }
    const epoch = lifetime;
    const current = connectionsState.getSnapshot();
    const retained: Record<string, GitNodeAvailability> = {};
    const pending: Promise<void>[] = [];
    for (const node of current.nodes) {
      const session = current.sessions[node.id];
      if (session?.phase !== ConnectionPhase.Connected) {
        probes.delete(node.id);
        continue;
      }
      const key = JSON.stringify([node.principal, session.generation, refreshVersion]);
      const previous = state.getSnapshot().nodes[node.id];
      if (previous?.principal === node.principal && previous.generation === session.generation) {
        retained[node.id] = previous;
      }
      if (probes.get(node.id) === key) {
        continue;
      }
      probes.set(node.id, key);
      retained[node.id] = {
        principal: node.principal,
        generation: session.generation,
        available: false,
        error: null,
      };
      pending.push(probe(node, session, key, epoch));
    }
    state.update((value) => ({ ...value, nodes: retained }));
    await Promise.all(pending);
  }
  return {
    state,
    activate() {
      if (active) {
        throw new Error("The Git presentation is already active.");
      }
      active = true;
      lifetime += 1;
      state.set({ enabled: true, nodes: {} });
      const detach = connectionsState.subscribe(() => {
        observe().catch(console.error);
      });
      observe().catch(console.error);
      return () => {
        active = false;
        lifetime += 1;
        detach();
        probes.clear();
        state.set({ enabled: false, nodes: {} });
      };
    },
    async refresh() {
      refreshVersion += 1;
      probes = new Map();
      await observe();
    },
  };
}

/** Tembra's last consumer Reference owns discovery subscriptions and their cleanup. */
export function gitIntegrationOrb(presentation: ReturnType<typeof createGitPresentation>) {
  return orb({
    name: "swarmie.git.integration",
    apiVersion: 1,
    services: {
      "swarmie.git.integration": {
        create(self) {
          self.resource("availability", {
            open: () => presentation.activate(),
            close: (dispose) => {
              dispose();
            },
          });
          return {};
        },
        methods: {},
      },
    },
  });
}
