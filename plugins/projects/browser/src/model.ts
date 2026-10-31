export type Project = { id: string; name: string; description: string; archived: boolean };
export type PersonalFolders = { projectId: string; nodeId: string; paths: string[] };
export type ProjectsData = {
  records: Project[];
  links: Record<string, string>;
  personalFolders: PersonalFolders[];
};
export type ProjectsCommand =
  | { kind: "create"; id: string; name: string; description: string }
  | { kind: "update"; id: string; name: string; description: string }
  | { kind: "archive"; id: string; archived: boolean }
  | { kind: "assign"; conversation: string; project: string }
  | { kind: "unassign"; conversation: string }
  | { kind: "folders"; project: string; node: string; paths: string[] }
  | { kind: "import"; data: ProjectsData & { revision: number } };
export type ProjectsBackend = {
  change: (command: ProjectsCommand) => Promise<{ revision: number }>;
  read: (minimumRevision?: number) => Promise<ProjectsData & { revision: number }>;
};
export type ProjectsState = ProjectsData & {
  revision: number;
  generation: number;
  available: boolean;
  nodeId: string | null;
  principal: string | null;
  dialog: { projectId: string | null } | null;
  draftLinks: Record<string, string>;
  error: string | null;
};
export const emptyProjects = (): ProjectsData => ({ records: [], links: {}, personalFolders: [] });
