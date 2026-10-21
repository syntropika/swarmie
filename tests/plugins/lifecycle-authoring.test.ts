import { expect, test } from "bun:test";

import { hookHandler } from "@tembra/sdk";

import {
  onTaskCreated,
  onTaskPhaseStarted,
  onTaskPhaseSettled,
  onTaskCheckpoint,
  onTaskWaiting,
  onTaskSleeping,
  onTaskRecovered,
  onTaskBlocked,
  onTaskParked,
  onTaskRetry,
  onTaskCancellationRequested,
  onTaskTerminal,
} from "../../plugins/durable-tasks/typescript";
import type { TaskCheckpoint, TaskPhaseSettled } from "../../plugins/durable-tasks/typescript";

const hooks = [
  ["task-created-hook", onTaskCreated],
  ["task-phase-started-hook", onTaskPhaseStarted],
  ["task-phase-settled-hook", onTaskPhaseSettled],
  ["task-checkpoint-hook", onTaskCheckpoint],
  ["task-waiting-hook", onTaskWaiting],
  ["task-sleeping-hook", onTaskSleeping],
  ["task-recovered-hook", onTaskRecovered],
  ["task-blocked-hook", onTaskBlocked],
  ["task-parked-hook", onTaskParked],
  ["task-retry-hook", onTaskRetry],
  ["task-cancellation-requested-hook", onTaskCancellationRequested],
  ["task-terminal-hook", onTaskTerminal],
] as const;

for (const [name, hook] of hooks) {
  test(`${name} has exact Rust and generated TypeScript descriptor parity`, () => {
    const exported = Bun.spawnSync(
      [
        "cargo",
        "+1.99.0",
        "run",
        "--locked",
        "--quiet",
        "-p",
        "swarmie-task-contracts",
        "--bin",
        "task-contract",
        "--",
        name,
      ],
      { cwd: new URL("../..", import.meta.url).pathname },
    );
    if (exported.exitCode !== 0) {
      throw new Error(`Canonical hook export failed: ${exported.stderr.toString()}`);
    }
    const descriptor: unknown = JSON.parse(exported.stdout.toString());
    expect(descriptor).toEqual(hook.descriptor);
    expect(hook.descriptor.strategy).toBe("series");
    expect(hook.descriptor.result).toBeNull();
  });
}

test("lifecycle hooks have independent Registry identities", () => {
  const names = hooks.map(([, hook]) => hook.descriptor.name);
  expect(new Set(names).size).toBe(12);
});

// Each callback receives its specialized canonical DTO without event filtering.
hookHandler(onTaskCheckpoint, (_context, { input }) => {
  const event: TaskCheckpoint = input;
  const revision: number = event.transition.task.revision;
  const committed: boolean = event.transition.checkpoint_committed;
  expect(revision).toBeDefined();
  expect(committed).toBeDefined();
});

hookHandler(onTaskPhaseSettled, (_context, { input }) => {
  const event: TaskPhaseSettled = input;
  const invocation: "run" | "migrate" = event.invocation;
  expect(invocation).toBeDefined();
});
