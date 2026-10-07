import { expect, it } from "vitest";
import { defaultNotifications, notificationFor } from "./notifications";
import { newThread } from "./protocol";
it("notification preferences suppress foreground, disabled and unrelated events", () => {
  const thread = {
    ...newThread("p", "w", "codex"),
    sessionId: "s",
    title: "Task",
  };
  const event = { sessionId: "s", sequence: 1, type: "turn.ended", data: {} };
  expect(
    notificationFor(thread, event, defaultNotifications, false),
  ).toBeUndefined();
  expect(
    notificationFor(
      thread,
      event,
      { ...defaultNotifications, enabled: true },
      true,
    ),
  ).toBeUndefined();
  expect(
    notificationFor(
      thread,
      { ...event, type: "text.delta" },
      { ...defaultNotifications, enabled: true },
      false,
    ),
  ).toBeUndefined();
});
it("input and completion events use stable identities without exposing answers or raw provider data", () => {
  const thread = {
    ...newThread("p", "w", "codex"),
    sessionId: "s",
    title: "Task",
  };
  expect(
    notificationFor(
      thread,
      {
        sessionId: "s",
        sequence: 2,
        type: "permission.requested",
        data: { secret: "Never expose" },
      },
      { ...defaultNotifications, enabled: true },
      false,
    ),
  ).toEqual({
    id: "mpai-s-2",
    title: "Your agent needs an answer",
    body: "Task",
  });
});
