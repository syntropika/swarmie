import type { Event, Thread } from "./protocol";
import type { LocalStoreIdentity } from "../../../../packages/clients/typescript/src/generated";
import { nodeKey } from "./nodeScope";
export type NotificationPreferences = {
  enabled: boolean;
  completion: boolean;
  input: boolean;
  failure: boolean;
  backgroundOnly: boolean;
};
const key = "mpai.notifications.v1";
export const defaultNotifications: NotificationPreferences = {
  enabled: false,
  completion: true,
  input: true,
  failure: true,
  backgroundOnly: true,
};
export function readNotificationPreferences(): NotificationPreferences {
  try {
    return {
      ...defaultNotifications,
      ...JSON.parse(localStorage.getItem(key) ?? "{}"),
    } as NotificationPreferences;
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
  if (!preferences.enabled || (preferences.backgroundOnly && focused)) return;
  const kind =
    event.type === "permission.requested" && preferences.input
      ? "Your agent needs an answer"
      : event.type === "error" && preferences.failure
        ? "Agent encountered an error"
        : event.type === "turn.ended" && preferences.completion
          ? "Agent finished"
          : undefined;
  if (!kind) return;
  return {
    id: `mpai-${thread.sessionId}-${event.sequence}`,
    title: kind,
    body: thread.title.slice(0, 240),
  };
}
export class NotificationService {
  private seen = new Set<string>();
  async configure(preferences: NotificationPreferences) {
    if (preferences.enabled && typeof Notification === "undefined")
      throw new Error("Notifications are unavailable in this WebView.");
    localStorage.setItem(key, JSON.stringify(preferences));
    if (
      preferences.enabled &&
      typeof Notification !== "undefined" &&
      Notification.permission === "default"
    )
      await Notification.requestPermission();
  }
  async events(
    thread: Thread,
    events: Event[],
    preferences: NotificationPreferences,
    node: LocalStoreIdentity,
  ) {
    for (const event of events) {
      const payload = notificationFor(
        thread,
        event,
        preferences,
        document.hasFocus(),
      );
      if (!payload) continue;
      const id = JSON.stringify([nodeKey(node), payload.id]);
      if (this.seen.has(id)) continue;
      this.seen.add(id);
      if (this.seen.size > 512)
        this.seen.delete(this.seen.values().next().value!);
      if (
        typeof Notification !== "undefined" &&
        Notification.permission === "granted"
      )
        new Notification(payload.title, {
          body: payload.body,
          tag: id,
        });
    }
  }
}
