import type { PersonalFolders, Project, ProjectsData, ProjectsPersistence } from "./index";
import { emptyProjects } from "./index";

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

/** The embedding supplies the current local profile/account key; it is not a workspace key. */
export function browserProjectsPersistence(profileKey: string): ProjectsPersistence {
  const key = `swarmie.projects.v1.${encodeURIComponent(profileKey)}`;
  return {
    load() {
      const stored = localStorage.getItem(key);
      if (stored === null) {
        return emptyProjects();
      }
      const value: unknown = JSON.parse(stored);
      return decodeProjects(value);
    },
    save(data) {
      localStorage.setItem(key, JSON.stringify(data));
    },
  };
}
