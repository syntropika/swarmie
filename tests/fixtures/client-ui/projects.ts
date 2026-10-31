import { emptyProjects } from "../../../plugins/projects/browser/src/index";
import type { ProjectsBackend, ProjectsData } from "../../../plugins/projects/browser/src/index";

/** Synthetic presentation backend. Native storage and validation are tested in Rust. */
export function fixtureProjects(initial: ProjectsData = emptyProjects()) {
  let value = {...initial, revision: 0};
  const backend: ProjectsBackend & { snapshot: () => typeof value } = {
    snapshot: () => value,
    read: () => Promise.resolve(value),
    change(command) {
      let next = structuredClone(value);
      switch (command.kind) {
        case "create":
          next.records.push({id: command.id, name: command.name, description: command.description, archived: false});
          break;
        case "update":
          next.records = next.records.map((record) => record.id === command.id ? {...record, name: command.name, description: command.description} : record);
          break;
        case "archive":
          next.records = next.records.map((record) => record.id === command.id ? {...record, archived: command.archived} : record);
          break;
        case "assign":
          next.links[command.conversation] = command.project;
          break;
        case "unassign":
          delete next.links[command.conversation];
          break;
        case "folders":
          next.personalFolders = next.personalFolders.filter((item) => item.projectId !== command.project || item.nodeId !== command.node);
          if (command.paths.length) {
            next.personalFolders.push({projectId: command.project, nodeId: command.node, paths: [...new Set(command.paths)]});
          }
          break;
        case "import":
          next = {...command.data, revision: next.revision};
          break;
      }
      next.revision += 1;
      value = next;
      return Promise.resolve(value);
    },
  };
  return backend;
}
