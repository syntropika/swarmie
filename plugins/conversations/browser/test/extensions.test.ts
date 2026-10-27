import { expect, test } from "bun:test";

import { createConversationsUi } from "../src/extensions";

const icon = () => null;
test("owner slots publish stable ordered snapshots and exact once cleanup", () => {
  const api = createConversationsUi();
  const before = api.composer.actions.getSnapshot();
  expect(api.composer.actions.getSnapshot()).toBe(before);
  let changes = 0;
  const detach = api.composer.actions.subscribe(() => {
    changes += 1;
  });
  const late = api.composer.registerAction({
    id: "later",
    owner: "one",
    label: "Later",
    icon,
    slot: "before-send",
    order: 20,
    execute: () => undefined,
  });
  const early = api.composer.registerAction({
    id: "earlier",
    owner: "two",
    label: "Earlier",
    icon,
    slot: "before-send",
    order: 10,
    execute: () => undefined,
  });
  expect(api.composer.actions.getSnapshot().map((action) => action.id)).toEqual([
    "earlier",
    "later",
  ]);
  late();
  late();
  expect(changes).toBe(3);
  expect(api.composer.actions.getSnapshot().map((action) => action.id)).toEqual(["earlier"]);
  early();
  detach();
  api.dispose();
  expect(() =>
    api.composer.registerAction({
      id: "gone",
      owner: "gone",
      label: "Gone",
      icon,
      slot: "leading",
      execute: () => undefined,
    }),
  ).toThrow();
});
