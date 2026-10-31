import type { PersonalFolders, Project, ProjectsData } from "./model";

function object(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}
function strings(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((entry: unknown) => typeof entry === "string");
}
function project(value: unknown): value is Project {
  return (
    object(value) &&
    typeof value.id === "string" &&
    typeof value.name === "string" &&
    typeof value.description === "string" &&
    typeof value.archived === "boolean"
  );
}
function folders(value: unknown): value is PersonalFolders {
  return (
    object(value) &&
    typeof value.projectId === "string" &&
    typeof value.nodeId === "string" &&
    strings(value.paths)
  );
}
export function decodeProjects(value: unknown): ProjectsData {
  if (
    !object(value) ||
    !Array.isArray(value.records) ||
    !value.records.every(project) ||
    !object(value.links) ||
    !Object.values(value.links).every((entry) => typeof entry === "string") ||
    !Array.isArray(value.personalFolders) ||
    !value.personalFolders.every(folders)
  ) {
    throw new Error("Saved projects could not be read. The existing data has been preserved.");
  }
  return {
    records: value.records,
    links: Object.fromEntries(
      Object.entries(value.links).filter(
        (entry): entry is [string, string] => typeof entry[1] === "string",
      ),
    ),
    personalFolders: value.personalFolders,
  };
}

/** Reads an older Browser export without writing or deleting the source. Import is explicit. */
export function savedBrowserProjects(profileKey: string): ProjectsData | null {
  const key = `swarmie.projects.v1.${encodeURIComponent(profileKey)}`;
  const stored = localStorage.getItem(key);
  return stored === null ? null : decodeProjects(JSON.parse(stored));
}

export function decodeProjectSnapshot(value: unknown): ProjectsData & { revision: number } {
  const data = decodeProjects(value);
  if (
    !object(value) ||
    typeof value.revision !== "number" ||
    !Number.isSafeInteger(value.revision) ||
    value.revision < 0
  ) {
    throw new Error("The node returned an invalid project revision.");
  }
  return { ...data, revision: value.revision };
}
