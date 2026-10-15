import { expect, it, vi } from "vitest";

import {
  defaultNotifications,
  notificationFor,
  NotificationService,
} from "~/web/src/notifications";
import { newThread } from "~/web/src/protocol";

it("notification preferences suppress foreground, disabled and unrelated events", () => {
  const thread = {
    ...newThread("p", "w", "codex"),
    sessionId: "s",
    title: "Task",
  };
  const event = { sessionId: "s", sequence: 1, type: "turn.ended", data: {} };
  expect(notificationFor(thread, event, defaultNotifications, false)).toBeUndefined();
  expect(
    notificationFor(thread, event, { ...defaultNotifications, enabled: true }, true),
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
    id: "swarmie-s-2",
    title: "Your agent needs an answer",
    body: "Task",
  });
});

it("keeps notification tags separate for equal session IDs and sequences on different nodes", () => {
  const notify = vi.fn<(title: string, options: NotificationOptions) => void>();
  class NotificationFixture extends EventTarget {
    static readonly permission = "granted";
    constructor(title: string, options: NotificationOptions) {
      super();
      notify(title, options);
    }
  }
  vi.stubGlobal("Notification", NotificationFixture);
  vi.stubGlobal("document", { hasFocus: () => false });
  try {
    const thread = { ...newThread("p", "w", "codex"), sessionId: "session", title: "Task" };
    const event = { sessionId: "session", sequence: 1, type: "turn.ended", data: {} };
    const service = new NotificationService();
    const preferences = { ...defaultNotifications, enabled: true };
    const first = { environmentId: "a", storeId: "store", ownerScopeId: "owner" };
    const second = { ...first, environmentId: "b" };
    service.events(thread, [event], preferences, first);
    service.events(thread, [event], preferences, second);
    service.events(thread, [event], preferences, first);
    expect(notify).toHaveBeenCalledTimes(2);
    expect(notify.mock.calls[0][1].tag).not.toBe(notify.mock.calls[1][1].tag);
  } finally {
    vi.unstubAllGlobals();
  }
});
