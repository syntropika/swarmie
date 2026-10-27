import type { Snapshot } from "@swarmie/client-ui";
import type { ConversationSummary } from "@swarmie/plugin-conversations-browser";
import { createContext, useContext } from "react";

import type { ProjectsApi } from "./index";

export type ProjectsPresentation = {
  api: ProjectsApi;
  conversations: Snapshot<readonly ConversationSummary[]>;
  nodes: Snapshot<readonly { id: string; label: string }[]>;
  open: (projectId?: string) => void;
  openConversation: (conversationId: string) => void;
  createConversation: (projectId: string) => Promise<void>;
};
export const ProjectsContext = createContext<ProjectsPresentation | null>(null);
export function useProjects() {
  const value = useContext(ProjectsContext);
  if (value === null) {
    throw new Error("Projects presentation is not mounted.");
  }
  return value;
}
