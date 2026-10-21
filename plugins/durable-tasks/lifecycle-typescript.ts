import { Type, defineHook } from "@tembra/sdk";

import { TaskBlockedObserver } from "../../target/generated/contracts/task-blocked-observer";
import type { TaskBlockedObserverReference } from "../../target/generated/contracts/task-blocked-observer";
import { TaskCancellationRequestedObserver } from "../../target/generated/contracts/task-cancellation-requested-observer";
import type { TaskCancellationRequestedObserverReference } from "../../target/generated/contracts/task-cancellation-requested-observer";
import { TaskCheckpointObserver } from "../../target/generated/contracts/task-checkpoint-observer";
import type { TaskCheckpointObserverReference } from "../../target/generated/contracts/task-checkpoint-observer";
import { TaskCreatedObserver } from "../../target/generated/contracts/task-created-observer";
import type { TaskCreatedObserverReference } from "../../target/generated/contracts/task-created-observer";
import { TaskParkedObserver } from "../../target/generated/contracts/task-parked-observer";
import type { TaskParkedObserverReference } from "../../target/generated/contracts/task-parked-observer";
import { TaskPhaseSettledObserver } from "../../target/generated/contracts/task-phase-settled-observer";
import type { TaskPhaseSettledObserverReference } from "../../target/generated/contracts/task-phase-settled-observer";
import { TaskPhaseStartedObserver } from "../../target/generated/contracts/task-phase-started-observer";
import type { TaskPhaseStartedObserverReference } from "../../target/generated/contracts/task-phase-started-observer";
import { TaskRecoveredObserver } from "../../target/generated/contracts/task-recovered-observer";
import type { TaskRecoveredObserverReference } from "../../target/generated/contracts/task-recovered-observer";
import { TaskRetryObserver } from "../../target/generated/contracts/task-retry-observer";
import type { TaskRetryObserverReference } from "../../target/generated/contracts/task-retry-observer";
import { TaskSleepingObserver } from "../../target/generated/contracts/task-sleeping-observer";
import type { TaskSleepingObserverReference } from "../../target/generated/contracts/task-sleeping-observer";
import { TaskTerminalObserver } from "../../target/generated/contracts/task-terminal-observer";
import type { TaskTerminalObserverReference } from "../../target/generated/contracts/task-terminal-observer";
import { TaskWaitingObserver } from "../../target/generated/contracts/task-waiting-observer";
import type { TaskWaitingObserverReference } from "../../target/generated/contracts/task-waiting-observer";

/** Canonical Rust inputs, generated through Tembra's Contract tooling. */
export type TaskCreated = Parameters<TaskCreatedObserverReference["observe"]>[0];
export type TaskPhaseStarted = Parameters<TaskPhaseStartedObserverReference["observe"]>[0];
export type TaskPhaseSettled = Parameters<TaskPhaseSettledObserverReference["observe"]>[0];
export type TaskCheckpoint = Parameters<TaskCheckpointObserverReference["observe"]>[0];
export type TaskWaiting = Parameters<TaskWaitingObserverReference["observe"]>[0];
export type TaskSleeping = Parameters<TaskSleepingObserverReference["observe"]>[0];
export type TaskRecovered = Parameters<TaskRecoveredObserverReference["observe"]>[0];
export type TaskBlocked = Parameters<TaskBlockedObserverReference["observe"]>[0];
export type TaskParked = Parameters<TaskParkedObserverReference["observe"]>[0];
export type TaskRetry = Parameters<TaskRetryObserverReference["observe"]>[0];
export type TaskCancellationRequested = Parameters<
  TaskCancellationRequestedObserverReference["observe"]
>[0];
export type TaskTerminal = Parameters<TaskTerminalObserverReference["observe"]>[0];

function portableInput<T>(contract: {
  readonly variants: Record<string, unknown>;
}): ReturnType<typeof Type.Unsafe<T>> {
  const variant = contract.variants[""];
  if (typeof variant !== "object" || variant === null || !("observe" in variant)) {
    throw new TypeError("Task lifecycle requires its generated observer method");
  }
  const member = variant.observe;
  if (
    typeof member !== "object" ||
    member === null ||
    !("kind" in member) ||
    member.kind !== "method" ||
    !("args" in member) ||
    typeof member.args !== "object" ||
    member.args === null ||
    "native" in member.args
  ) {
    throw new TypeError("Task lifecycle requires its generated portable input schema");
  }
  // TypeBox's hidden unsafe marker is not wire metadata.
  return { ...Type.Unsafe<T>(member.args) };
}

/** Independent versioned series hook; registration and execution belong to Tembra. */
export const onTaskCreated = defineHook("swarmie.task-created", {
  version: 1,
  strategy: "series",
  input: portableInput<TaskCreated>(TaskCreatedObserver.descriptor),
});

/** Independent versioned series hook; registration and execution belong to Tembra. */
export const onTaskPhaseStarted = defineHook("swarmie.task-phase-started", {
  version: 1,
  strategy: "series",
  input: portableInput<TaskPhaseStarted>(TaskPhaseStartedObserver.descriptor),
});

/** Independent versioned series hook; registration and execution belong to Tembra. */
export const onTaskPhaseSettled = defineHook("swarmie.task-phase-settled", {
  version: 1,
  strategy: "series",
  input: portableInput<TaskPhaseSettled>(TaskPhaseSettledObserver.descriptor),
});

/** Independent versioned series hook; registration and execution belong to Tembra. */
export const onTaskCheckpoint = defineHook("swarmie.task-checkpoint", {
  version: 1,
  strategy: "series",
  input: portableInput<TaskCheckpoint>(TaskCheckpointObserver.descriptor),
});

/** Independent versioned series hook; registration and execution belong to Tembra. */
export const onTaskWaiting = defineHook("swarmie.task-waiting", {
  version: 1,
  strategy: "series",
  input: portableInput<TaskWaiting>(TaskWaitingObserver.descriptor),
});

/** Independent versioned series hook; registration and execution belong to Tembra. */
export const onTaskSleeping = defineHook("swarmie.task-sleeping", {
  version: 1,
  strategy: "series",
  input: portableInput<TaskSleeping>(TaskSleepingObserver.descriptor),
});

/** Independent versioned series hook; registration and execution belong to Tembra. */
export const onTaskRecovered = defineHook("swarmie.task-recovered", {
  version: 1,
  strategy: "series",
  input: portableInput<TaskRecovered>(TaskRecoveredObserver.descriptor),
});

/** Independent versioned series hook; registration and execution belong to Tembra. */
export const onTaskBlocked = defineHook("swarmie.task-blocked", {
  version: 1,
  strategy: "series",
  input: portableInput<TaskBlocked>(TaskBlockedObserver.descriptor),
});

/** Independent versioned series hook; registration and execution belong to Tembra. */
export const onTaskParked = defineHook("swarmie.task-parked", {
  version: 1,
  strategy: "series",
  input: portableInput<TaskParked>(TaskParkedObserver.descriptor),
});

/** Independent versioned series hook; registration and execution belong to Tembra. */
export const onTaskRetry = defineHook("swarmie.task-retry", {
  version: 1,
  strategy: "series",
  input: portableInput<TaskRetry>(TaskRetryObserver.descriptor),
});

/** Independent versioned series hook; registration and execution belong to Tembra. */
export const onTaskCancellationRequested = defineHook("swarmie.task-cancellation-requested", {
  version: 1,
  strategy: "series",
  input: portableInput<TaskCancellationRequested>(TaskCancellationRequestedObserver.descriptor),
});

/** Independent versioned series hook; registration and execution belong to Tembra. */
export const onTaskTerminal = defineHook("swarmie.task-terminal", {
  version: 1,
  strategy: "series",
  input: portableInput<TaskTerminal>(TaskTerminalObserver.descriptor),
});
