import { Type, defineHook } from "@tembra/sdk";

import { TaskLifecycleObserver } from "../../target/generated/contracts/task-lifecycle-observer";
import type { TaskLifecycleObserverReference } from "../../target/generated/contracts/task-lifecycle-observer";

/** Canonical Rust DTO, generated through the existing Tembra Contract tools. */
export type TaskLifecycleEvent = Parameters<TaskLifecycleObserverReference["observe"]>[0];

const member = TaskLifecycleObserver.descriptor.variants[""]?.observe;
if (
  member?.kind !== "method" ||
  member.args === undefined ||
  typeof member.args !== "object" ||
  member.args === null ||
  "native" in member.args
) {
  throw new TypeError("Task lifecycle requires its generated portable input schema");
}

/** Versioned observation pipeline. Registration and physical execution belong to Tembra. */
export const TaskLifecycle = defineHook("swarmie.task-lifecycle", {
  version: 1,
  strategy: "series",
  // Copy portable fields only: TypeBox's hidden unsafe marker is not wire metadata.
  input: { ...Type.Unsafe<TaskLifecycleEvent>(member.args) },
});
