import type { Snapshot, UiStore } from "@swarmie/client-ui";
import { updatePresentation } from "@swarmie/client-ui";
import { orb } from "@syntropika/tembra-sdk";

export type Project = { id: string; name: string; description: string; archived: boolean };
export type PersonalFolders = { projectId: string; nodeId: string; paths: string[] };
export type ProjectsData = {
  records: Project[];
  links: Record<string, string>;
  personalFolders: PersonalFolders[];
};
export type ProjectsState = ProjectsData & {
  dialog: { projectId: string | null } | null;
  draftLinks: Record<string, string>;
  error: string | null;
};
export type ProjectsPersistence = { load: () => ProjectsData; save: (data: ProjectsData) => void };
export const emptyProjects = (): ProjectsData => ({ records: [], links: {}, personalFolders: [] });
export function projectsOrb(initial: ProjectsData = emptyProjects()) {
  return orb({
    name: "swarmie.projects.ui",
    apiVersion: 1,
    services: {
      "swarmie.projects.ui": {
        create: (self) =>
          self.state<ProjectsState>({ ...initial, draftLinks: {}, dialog: null, error: null }),
        methods: { update: updatePresentation<ProjectsState> },
      },
    },
  });
}
export type ProjectsApi = ReturnType<typeof createProjectsApi>;

/** Owns project commands and links; Conversations contains neither project data nor mutations. */
export function createProjectsApi(
  state: UiStore<Readonly<ProjectsState>>,
  commit: (change: (current: Readonly<ProjectsState>) => ProjectsState) => Promise<unknown>,
  nextId: () => string,
) {
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
  return {
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
      await commit((current) => ({ ...current, records: [...current.records, project] }));
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
      await commit((current) => ({
        ...current,
        records: current.records.map((project) =>
          project.id === projectId
            ? { ...project, name, description: changes.description.trim() }
            : project,
        ),
      }));
    },
    async stageConversation(conversationId: string, projectId: string | null) {
      if (projectId !== null && !present(projectId)) {
        throw new Error("The project is unavailable.");
      }
      return await commit((current) => ({
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
      const projectId = state.getSnapshot().draftLinks[conversationId];
      if (projectId === undefined) {
        return;
      }
      await commit((current) => ({
        ...current,
        links: { ...current.links, [conversationId]: projectId },
        draftLinks: Object.fromEntries(
          Object.entries(current.draftLinks).filter(([id]) => id !== conversationId),
        ),
      }));
    },
    async assignConversation(input: { conversationId: string; projectId: string }) {
      if (!present(input.projectId)) {
        throw new Error("The project is unavailable.");
      }
      await commit((current) => ({
        ...current,
        links: { ...current.links, [input.conversationId]: input.projectId },
      }));
    },
    async unassignConversation(conversationId: string) {
      await commit((current) => ({
        ...current,
        links: Object.fromEntries(
          Object.entries(current.links).filter(([id]) => id !== conversationId),
        ),
      }));
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
      const paths = [...new Set(input.paths.filter((path) => path !== ""))];
      await commit((current) => ({
        ...current,
        personalFolders: [
          ...current.personalFolders.filter(
            (entry) => entry.projectId !== projectId || entry.nodeId !== input.nodeId,
          ),
          ...(paths.length ? [{ projectId, nodeId: input.nodeId, paths }] : []),
        ],
      }));
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
