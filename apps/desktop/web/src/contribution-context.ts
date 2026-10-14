import { createContext, useContext } from "react";

import type { ContributionMounts, Mount } from "./contribution-mounts";
import type { ContributionRegistry, ClientExtension } from "./contributions";
import type { Thread } from "./protocol";
import type { WorkspaceStore } from "./workspace";

export type { Mount } from "./contribution-mounts";

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
