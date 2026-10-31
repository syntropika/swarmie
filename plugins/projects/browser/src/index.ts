import type { Snapshot, UiStore } from "@swarmie/client-ui";
import { updatePresentation } from "@swarmie/client-ui";
import { orb } from "@syntropika/tembra-sdk";

import { emptyProjects } from "./model";
import type {
  Project,
  ProjectsBackend,
  ProjectsCommand,
  ProjectsData,
  ProjectsState,
} from "./model";

export { emptyProjects } from "./model";

export type {
  Project,
  PersonalFolders,
  ProjectsBackend,
  ProjectsCommand,
  ProjectsData,
  ProjectsState,
} from "./model";

export function projectsOrb(initial: ProjectsData = emptyProjects()) {
  return orb({
    name: "swarmie.projects.ui",
    apiVersion: 1,
    services: {
      "swarmie.projects.ui": {
        create: (self) =>
          self.state<ProjectsState>({
            ...initial,
            revision: 0,
            generation: 0,
            available: false,
            nodeId: null,
            principal: null,
            draftLinks: {},
            dialog: null,
            error: null,
          }),
        methods: { update: updatePresentation<ProjectsState> },
      },
    },
  });
}
export type ProjectsApi = ReturnType<typeof createProjectsApi>;

/** Owns project commands and links; Conversations contains neither project data nor mutations. */
export function createProjectsApi(
  state: UiStore<Readonly<ProjectsState>> & {
    commit: (change: (current: Readonly<ProjectsState>) => ProjectsState) => Promise<unknown>;
  },
  backend: ProjectsBackend,
  nextId: () => string,
) {
  async function change(command: ProjectsCommand) {
    const { generation } = state.getSnapshot();
    const receipt = await backend.change(command);
    if (state.getSnapshot().generation !== generation) {
      return receipt;
    }
    try {
      const data = await backend.read(receipt.revision);
      if (data.revision < receipt.revision) {
        throw new Error("The project read predates the confirmed change.");
      }
      if (state.getSnapshot().generation === generation) {
        state.update((current) =>
          data.revision < current.revision ? current : { ...current, ...data },
        );
      }
    } catch (error: unknown) {
      if (state.getSnapshot().generation === generation) {
        state.update((current) => ({
          ...current,
          error: error instanceof Error ? error.message : String(error),
        }));
      }
    }
    return receipt;
  }
  let previousLinks = state.getSnapshot().links;
  let previousRecords = state.getSnapshot().records;
  let members: readonly string[] = [];
  const membership: Snapshot<readonly string[]> = {
    subscribe(changed) {
      let previous = membership.getSnapshot();
      return state.subscribe(() => {
        const current = membership.getSnapshot();
        if (previous !== current) {
          previous = current;
          changed();
        }
      });
    },
    getSnapshot() {
      const current = state.getSnapshot();
      if (previousLinks !== current.links || previousRecords !== current.records) {
        previousLinks = current.links;
        previousRecords = current.records;
        members = Object.entries(current.links)
          .filter(([, id]) =>
            current.records.some((project) => project.id === id && !project.archived),
          )
          .map(([id]) => id);
      }
      return members;
    },
  };
  previousLinks = {};
  function projectOf(conversationId: string): string | null {
    return state.getSnapshot().links[conversationId] ?? null;
  }
  function present(projectId: string) {
    return state.getSnapshot().records.some((entry) => entry.id === projectId && !entry.archived);
  }
  function scope() {
    const { generation } = state.getSnapshot();
    return () => state.getSnapshot().generation === generation;
  }
  return {
    scope,
    state,
    membership,
    projectOf,
    async create(input: { name: string; description?: string }) {
      const name = input.name.trim();
      if (!name) {
        throw new Error("Enter a project name.");
      }
      const project: Project = {
        id: nextId(),
        name,
        description: input.description?.trim() ?? "",
        archived: false,
      };
      await change({ kind: "create", id: project.id, name, description: project.description });
      return project;
    },
    async update(projectId: string, changes: { name: string; description: string }) {
      if (!present(projectId)) {
        throw new Error("The project is unavailable.");
      }
      const name = changes.name.trim();
      if (!name) {
        throw new Error("Enter a project name.");
      }
      await change({
        kind: "update",
        id: projectId,
        name,
        description: changes.description.trim(),
      });
    },
    async stageConversation(conversationId: string, projectId: string | null) {
      if (projectId !== null && !present(projectId)) {
        throw new Error("The project is unavailable.");
      }
      await state.commit((current) => ({
        ...current,
        draftLinks:
          projectId === null
            ? Object.fromEntries(
                Object.entries(current.draftLinks).filter(([id]) => id !== conversationId),
              )
            : { ...current.draftLinks, [conversationId]: projectId },
      }));
    },
    async admitDraft(conversationId: string) {
      const currentScope = scope();
      const projectId = state.getSnapshot().draftLinks[conversationId];
      if (projectId === undefined) {
        return;
      }
      await change({ kind: "assign", conversation: conversationId, project: projectId });
      if (!currentScope()) {
        return;
      }
      state.update((current) => ({
        ...current,
        draftLinks:
          current.draftLinks[conversationId] === projectId
            ? Object.fromEntries(
                Object.entries(current.draftLinks).filter(([id]) => id !== conversationId),
              )
            : current.draftLinks,
      }));
    },
    async assignConversation(input: { conversationId: string; projectId: string }) {
      if (!present(input.projectId)) {
        throw new Error("The project is unavailable.");
      }
      await change({
        kind: "assign",
        conversation: input.conversationId,
        project: input.projectId,
      });
    },
    async unassignConversation(conversationId: string) {
      await change({ kind: "unassign", conversation: conversationId });
    },
    listConversations(projectId: string, options: { cursor?: string; limit: number }) {
      const ids = Object.entries(state.getSnapshot().links)
        .filter(([, project]) => project === projectId)
        .map(([id]) => id)
        .toSorted();
      const start = options.cursor === undefined ? 0 : ids.indexOf(options.cursor) + 1;
      const items = ids.slice(start, start + Math.max(1, Math.min(options.limit, 100)));
      return { items, next: start + items.length < ids.length ? (items.at(-1) ?? null) : null };
    },
    async setPersonalFolders(projectId: string, input: { nodeId: string; paths: string[] }) {
      if (!present(projectId)) {
        throw new Error("The project is unavailable.");
      }
      await change({ kind: "folders", project: projectId, node: input.nodeId, paths: input.paths });
    },
    async archive(projectId: string, archived: boolean) {
      await change({ kind: "archive", id: projectId, archived });
    },
    async importData(data: ProjectsData) {
      await change({ kind: "import", data: { ...data, revision: 0 } });
    },
    showCreate: () => {
      state.update((current) => ({ ...current, dialog: { projectId: null }, error: null }));
    },
    showEdit: (projectId: string) => {
      state.update((current) => ({ ...current, dialog: { projectId }, error: null }));
    },
    closeDialog: () => {
      state.update((current) => ({ ...current, dialog: null }));
    },
    fail: (error: unknown) => {
      state.update((current) => ({
        ...current,
        error: error instanceof Error ? error.message : String(error),
      }));
    },
  };
}
export type ProjectsSource = Snapshot<Readonly<ProjectsState>>;

export type { DirectoryBrowser, DirectoryPage, DirectoryReader } from "./directories";

export { createNativeProjects } from "./native";
