import { activityOrb } from "@swarmie/plugin-activity-browser";
import { conversationsOrb } from "@swarmie/plugin-conversations-browser";
import type { ConversationsState } from "@swarmie/plugin-conversations-browser";
import type { Context, Declaration, Readable, ServiceReference } from "@syntropika/tembra-sdk";
import type { Host } from "@syntropika/tembra-sdk/runtime";

type Conversations = ServiceReference<
  (typeof conversationsOrb.services)["swarmie.conversations.ui"]
>;
type Activity = ServiceReference<(typeof activityOrb.services)["swarmie.activity.ui"]>;
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
  Assert<Equal<ReturnType<Activity["update"]>, Promise<void>>>,
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
  const activity = await host.get(activityOrb.services["swarmie.activity.ui"], "main");
  const published: Promise<void> = activity.update((current) => ({ ...current, error: null }));

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
  // @ts-expect-error Browser activity entry projection does not publish native inbox entries.
  const nativeMethod: keyof Activity = "publish";
  // @ts-expect-error The projection update requires the complete state shape.
  await activity.update(() => []);

  return {
    nativeMethod,
    held,
    dependency,
    scoped,
    error,
    cleared,
    updated,
    published,
    stateMember,
    method,
  };
}
