import { conversationsOrb } from "@swarmie/plugin-conversations-browser";
import type { ConversationsState } from "@swarmie/plugin-conversations-browser";
import { notificationsOrb } from "@swarmie/plugin-notifications-browser";
import type { Context, Declaration, Readable, ServiceReference } from "@syntropika/tembra-sdk";
import type { Host } from "@syntropika/tembra-sdk/runtime";

type Conversations = ServiceReference<
  (typeof conversationsOrb.services)["swarmie.conversations.ui"]
>;
type Notifications = ServiceReference<
  (typeof notificationsOrb.services)["swarmie.notifications.ui"]
>;
type Equal<A, B> = 0 extends 1 & (A | B)
  ? false
  : [A] extends [B]
    ? [B] extends [A]
      ? true
      : false
    : false;
type Assert<T extends true> = T;

export type ServiceAssertions = [
  Assert<Equal<Conversations["state"], Readable<ConversationsState>>>,
  Assert<Equal<ReturnType<Conversations["clearError"]>, Promise<void>>>,
  Assert<Equal<ReturnType<Notifications["publish"]>, Promise<void>>>,
];

/** Compile-only qualification of published SDK inference against real client service declarations. */
export async function qualifyServices(host: Host, self: Declaration, context: Context) {
  const service = conversationsOrb.services["swarmie.conversations.ui"];
  const reference = await host.get(service, "main");
  const held: Conversations = host.hold(service, "held");
  const dependency: Conversations = self.get(service, "dependency");
  const scoped: Conversations = await context.get(service, "scoped");
  const error: string | null = reference.state.value.error;
  const cleared: Promise<void> = reference.clearError(context);
  const updated: Promise<void> = reference.update((current) => ({ ...current, error: null }));
  const notifications = await host.get(
    notificationsOrb.services["swarmie.notifications.ui"],
    "main",
  );
  const published: Promise<void> = notifications.publish({
    owner: "conversations",
    entity: "main",
    title: "Complete",
    text: "Ready",
  });

  // @ts-expect-error State belongs to Conversations, not Views.
  const stateMember: keyof typeof reference.state.value = "tabs";
  // @ts-expect-error Readable State cannot be assigned directly.
  reference.state.value = { records: [], error: null };
  // @ts-expect-error Unknown methods are not exposed by typed references.
  const method: keyof typeof reference = "missing";
  // @ts-expect-error Implementation environment and Context are not business arguments.
  await reference.clearError("wrong");
  // @ts-expect-error The update callback must return the declared State shape.
  await reference.update(() => ({ records: "wrong", error: null }));
  // @ts-expect-error Notifications requires a structured input.
  await notifications.publish("wrong");
  // @ts-expect-error Business arguments remain separate from the trailing Context.
  await notifications.publish(context, { owner: "test", entity: "main", title: "", text: "" });

  return { held, dependency, scoped, error, cleared, updated, published, stateMember, method };
}
