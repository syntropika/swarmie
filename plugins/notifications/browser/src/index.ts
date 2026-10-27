import { orb } from "@syntropika/tembra-sdk";
import type { State } from "@syntropika/tembra-sdk";

export type Notification = {
  id: string;
  owner: string;
  entity: string;
  title: string;
  text: string;
  conversationId?: string;
  unread: boolean;
  updatedAt: number;
};
export type NotificationInput = Omit<Notification, "id" | "updatedAt" | "unread">;
/** Entity identity prevents repeated events from becoming a notification history. */
export function upsertNotification(
  current: readonly Notification[],
  input: NotificationInput,
  now: number,
): Notification[] {
  const id = JSON.stringify([input.owner, input.entity]);
  return [
    { ...input, id, unread: true, updatedAt: now },
    ...current.filter((item) => item.id !== id),
  ];
}
export const notificationsOrb = orb({
  name: "swarmie.notifications.ui",
  apiVersion: 1,
  services: {
    "swarmie.notifications.ui": {
      create: (self) => self.state<Notification[]>([]),
      methods: {
        publish(state: State<Notification[]>, context, input: NotificationInput) {
          state.update(context, (value) => upsertNotification(value, input, Date.now()));
        },
        read(state: State<Notification[]>, context, id: string) {
          state.update(context, (value) =>
            value.map((item) => (item.id === id ? { ...item, unread: false } : item)),
          );
        },
      },
    },
  },
});
