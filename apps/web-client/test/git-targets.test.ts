import { expect, test } from "bun:test";

import { gitDescriptor, gitTarget } from "../src/git/targets";

test("Git view descriptors keep the captured node, principal and checkout", () => {
  const descriptor = gitDescriptor({
    nodeId: "node-a",
    principal: "alice",
    path: "/projects/app",
    projectId: "project",
  });
  const tab = { ...descriptor, id: "tab", kind: "capability" as const };
  expect(gitTarget(tab)).toEqual({
    nodeId: "node-a",
    principal: "alice",
    path: "/projects/app",
    projectId: "project",
  });
  expect(gitTarget({ ...tab, target: { ...tab.target, principal: false } })).toBeNull();
  expect(gitTarget({ ...tab, target: { ...tab.target, path: "" } })).toBeNull();
  expect(gitTarget({ ...tab, owner: "another" })).toBeNull();
});
