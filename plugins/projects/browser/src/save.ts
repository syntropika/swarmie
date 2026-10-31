import type { ProjectsApi } from "./index";
import type { PersonalFolders } from "./model";

/** Keep a multi-command editor save and its UI continuations in the admitted client scope. */
export async function saveProjectChanges(
  api: ProjectsApi,
  input: {
    id: string | null;
    name: string;
    description: string;
    folders: readonly PersonalFolders[];
  },
  created: (id: string) => void,
) {
  const current = api.scope();
  const project = input.id === null ? await api.create(input) : { id: input.id };
  if (!current()) {
    return;
  }
  if (input.id !== null) {
    await api.update(input.id, input);
  }
  if (!current()) {
    return;
  }
  await Promise.all(
    input.folders.map(async (entry) => {
      if (current()) {
        await api.setPersonalFolders(project.id, entry);
      }
    }),
  );
  if (!current()) {
    return;
  }
  api.closeDialog();
  if (input.id === null) {
    created(project.id);
  }
}
