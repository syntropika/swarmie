import type { CapabilityTab } from "@swarmie/plugin-browser/views";

export type GitTarget = {
  nodeId: string;
  principal: string;
  path: string;
  projectId?: string;
};

/** A tab never follows a later node selection or a replacement principal. */
export function gitTarget(tab: CapabilityTab): GitTarget | null {
  const { nodeId, principal, path, projectId } = tab.target;
  if (
    tab.owner !== "git" ||
    tab.view !== "repository" ||
    typeof nodeId !== "string" ||
    typeof principal !== "string" ||
    typeof path !== "string" ||
    path.trim() === "" ||
    (projectId !== undefined && typeof projectId !== "string")
  ) {
    return null;
  }
  return { nodeId, principal, path, ...(typeof projectId === "string" ? { projectId } : {}) };
}

export function gitDescriptor(target: GitTarget) {
  return {
    owner: "git",
    view: "repository",
    title: `Files · ${
      target.path
        .split(/[\\/]/u)
        .toReversed()
        .find((part) => part !== "") ?? target.path
    }`,
    target: {
      nodeId: target.nodeId,
      principal: target.principal,
      path: target.path,
      ...(target.projectId !== undefined && target.projectId !== ""
        ? { projectId: target.projectId }
        : {}),
    },
  };
}
