import { expect, test } from "bun:test";

import { decodeActivity } from "../src/index";

test("activity entry projection validates native inbox identity and revisions", () => {
  const item = {
    id: JSON.stringify(["tools", "one"]),
    owner: "tools",
    entity: "one",
    title: "Complete",
    text: "Ready",
    unread: true,
    updatedAt: 1,
  };
  expect(decodeActivity({ revision: 2, items: [item] })).toEqual({
    revision: 2,
    items: [item],
  });
  expect(() => decodeActivity({ revision: -1, items: [] })).toThrow();
  expect(() => decodeActivity({ revision: 2, items: [{ ...item, id: "wrong" }] })).toThrow();
});
