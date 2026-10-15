import type { LocalStoreIdentity } from "@swarmie/contracts";

import { isRecord } from "~/src/checked-json";
import { nodeKey } from "~/src/node-scope";
import type { Event, Thread } from "~/src/protocol";

export type NotificationPreferences = {
  enabled: boolean;
  completion: boolean;
  input: boolean;
  failure: boolean;
  backgroundOnly: boolean;
};
const key = "swarmie.notifications.v1";
export const defaultNotifications: NotificationPreferences = {
  enabled: false,
  completion: true,
  input: true,
  failure: true,
  backgroundOnly: true,
};
export function readNotificationPreferences(): NotificationPreferences {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(key) ?? "{}");
    const result = { ...defaultNotifications };
    if (isRecord(saved)) {
      result.enabled = typeof saved.enabled === "boolean" ? saved.enabled : result.enabled;
      result.completion =
        typeof saved.completion === "boolean" ? saved.completion : result.completion;
      result.input = typeof saved.input === "boolean" ? saved.input : result.input;
      result.failure = typeof saved.failure === "boolean" ? saved.failure : result.failure;
      result.backgroundOnly =
        typeof saved.backgroundOnly === "boolean" ? saved.backgroundOnly : result.backgroundOnly;
    }
    return result;
  } catch {
    return defaultNotifications;
  }
}
export function notificationFor(
  thread: Thread,
  event: Event,
  preferences: NotificationPreferences,
  focused: boolean,
): { id: string; title: string; body: string } | undefined {
  if (!preferences.enabled || (preferences.backgroundOnly && focused)) {
    return undefined;
  }
  let kind: string | undefined;
  if (event.type === "permission.requested" && preferences.input) {
    kind = "Your agent needs an answer";
  } else if (event.type === "error" && preferences.failure) {
    kind = "Agent encountered an error";
  } else if (event.type === "turn.ended" && preferences.completion) {
    kind = "Agent finished";
  }
  if (kind === undefined) {
    return undefined;
  }
  return {
    id: `swarmie-${thread.sessionId}-${event.sequence}`,
    title: kind,
    body: thread.title.slice(0, 240),
  };
}
export class NotificationService {
  private readonly seen = new Set<string>();
  private readonly visible = new Set<Notification>();
  configure = configureNotifications;
  events(
    thread: Thread,
    events: Event[],
    preferences: NotificationPreferences,
    node: LocalStoreIdentity,
  ) {
    for (const event of events) {
      const payload = notificationFor(thread, event, preferences, document.hasFocus());
      if (!payload) {
        continue;
      }
      const id = JSON.stringify([nodeKey(node), payload.id]);
      if (this.seen.has(id)) {
        continue;
      }
      this.seen.add(id);
      if (this.seen.size > 512) {
        const oldest = this.seen.values().next();
        if (oldest.done !== true) {
          this.seen.delete(oldest.value);
        }
      }
      if (typeof Notification !== "undefined" && Notification.permission === "granted") {
        const display = new Notification(payload.title, { body: payload.body, tag: id });
        this.visible.add(display);
        display.addEventListener(
          "close",
          () => {
            this.visible.delete(display);
          },
          { once: true },
        );
      }
    }
  }
}

async function configureNotifications(preferences: NotificationPreferences) {
  if (preferences.enabled && typeof Notification === "undefined") {
    throw new Error("Notifications are unavailable in this WebView.");
  }
  localStorage.setItem(key, JSON.stringify(preferences));
  if (
    preferences.enabled &&
    typeof Notification !== "undefined" &&
    Notification.permission === "default"
  ) {
    await Notification.requestPermission();
  }
}
