import { expect, test } from "bun:test";

import { upsertNotification } from "../src/index";

test("a plugin notification replaces and promotes the same entity without adding history", () => {
  const first = { owner: "conversation", entity: "one", title: "One", text: "Ready" };
  const second = { owner: "conversation", entity: "two", title: "Two", text: "Ready" };
  let notifications = upsertNotification([], first, 1);
  notifications = upsertNotification(notifications, second, 2);
  notifications = upsertNotification(notifications, { ...first, text: "Updated" }, 3);
  expect(notifications).toHaveLength(2);
  expect(notifications.map((item) => item.entity)).toEqual(["one", "two"]);
  expect(notifications[0]?.text).toBe("Updated");
  const otherPlugin = upsertNotification(notifications, { ...first, owner: "another-plugin" }, 4);
  expect(otherPlugin).toHaveLength(3);
});
