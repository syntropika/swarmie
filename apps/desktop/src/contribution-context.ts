import { createContext, useContext } from "react";

import type { ContributionMounts, Mount } from "~/src/contribution-mounts";
import type { ContributionRegistry, ClientExtension } from "~/src/contributions";
import type { Thread } from "~/src/protocol";
import type { WorkspaceStore } from "~/src/workspace";

export type { Mount } from "~/src/contribution-mounts";

export type ClientContribution = {
  extension: ClientExtension;
  mounts?: Map<string, Mount>;
};
export type HostContext = {
  registry: ContributionRegistry;
  mounts: ContributionMounts;
  bindComposer: (
    id: string,
    insert: (text: string, options?: { separateWords?: boolean }) => void,
  ) => () => void;
  store: WorkspaceStore;
  thread: Thread;
};
export const Host = createContext<HostContext | null>(null);
export const useContributions = () => useContext(Host);
