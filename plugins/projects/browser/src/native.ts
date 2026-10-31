import type { Snapshot, UiStore } from "@swarmie/client-ui";
import { observeNodeService } from "@swarmie/plugin-browser/connections";
import type { Connections, ConnectionsState } from "@swarmie/plugin-browser/connections";

import { emptyProjects } from "./model";
import type { ProjectsBackend, ProjectsState } from "./model";
import { decodeProjectSnapshot } from "./persistence";

export function createNativeProjects(
  state: UiStore<Readonly<ProjectsState>>,
  nodes: Snapshot<Readonly<ConnectionsState>>,
  connections: Connections,
  provided?: ProjectsBackend,
) {
  if (provided !== undefined) {
    return {
      backend: provided,
      close: () => {
        /* The embedding owns the supplied backend. */
      },
    };
  }
  const binding = observeNodeService(nodes, connections, "swarmie.projects", {
    reset(nodeId, principal) {
      state.update((current) => {
        const sameScope = current.nodeId === nodeId && current.principal === principal;
        return {
          ...current,
          ...(sameScope ? {} : { ...emptyProjects(), revision: 0, draftLinks: {}, dialog: null }),
          generation: current.generation + 1,
          available: false,
          nodeId,
          principal,
          error: null,
        };
      });
    },
    receive(value) {
      const snapshot = decodeProjectSnapshot(value);
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
  const backend: ProjectsBackend = {
    async read(minimumRevision = 0) {
      return decodeProjectSnapshot(await binding.read(minimumRevision));
    },
    async change(command) {
      const value = await binding.invoke("change", command);
      if (
        value === null ||
        typeof value !== "object" ||
        !("revision" in value) ||
        typeof value.revision !== "number"
      ) {
        throw new Error("The native project receipt is invalid.");
      }
      return { revision: value.revision };
    },
  };
  return {
    backend,
    close: () => {
      binding.close();
    },
  };
}
