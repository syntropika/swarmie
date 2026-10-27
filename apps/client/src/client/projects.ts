import type { Snapshot } from "@swarmie/client-ui";
import { errorMessage } from "@swarmie/client-ui";
import { emptyProjects } from "@swarmie/plugin-projects-browser";
import type {
  ProjectsData,
  ProjectsPersistence,
  ProjectsState,
} from "@swarmie/plugin-projects-browser";

export function loadProjects(persistence: ProjectsPersistence | undefined) {
  try {
    return { data: persistence?.load() ?? emptyProjects(), error: null };
  } catch (error: unknown) {
    return { data: emptyProjects(), error: errorMessage(error) };
  }
}

export function persistProjects(
  state: Snapshot<Readonly<ProjectsState>>,
  persistence: ProjectsPersistence,
  failed: (error: unknown) => void,
) {
  let previous: ProjectsData = state.getSnapshot();
  return state.subscribe(() => {
    const current = state.getSnapshot();
    if (
      previous.records === current.records &&
      previous.links === current.links &&
      previous.personalFolders === current.personalFolders
    ) {
      return;
    }
    previous = current;
    try {
      persistence.save({
        records: current.records,
        links: current.links,
        personalFolders: current.personalFolders,
      });
    } catch (error: unknown) {
      failed(error);
    }
  });
}
