import { expect, test } from "bun:test";

import { hookHandler } from "@tembra/sdk";

import { TaskLifecycle } from "../../plugins/durable-tasks/typescript";
import type { TaskLifecycleEvent } from "../../plugins/durable-tasks/typescript";

test("Rust and generated TypeScript share the exact lifecycle hook descriptor", () => {
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
      "task-lifecycle-hook",
    ],
    { cwd: new URL("../..", import.meta.url).pathname },
  );
  expect(exported.exitCode).toBe(0);
  const descriptor: unknown = JSON.parse(exported.stdout.toString());
  expect(descriptor).toEqual(TaskLifecycle.descriptor);
  expect(TaskLifecycle.descriptor.strategy).toBe("series");
  expect(TaskLifecycle.descriptor.result).toBeNull();
});

// The public callback input comes from canonical generated bindings, including discrimination.
hookHandler(TaskLifecycle, (_context, { input }) => {
  const event: TaskLifecycleEvent = input;
  if (event.type === "committed") {
    const revision: number = event.task.revision;
    const checkpointCommitted: boolean = event.checkpoint_committed;
    expect(revision).toBeDefined();
    expect(checkpointCommitted).toBeDefined();
  } else {
    const invocation: "run" | "migrate" = event.invocation;
    expect(invocation).toBeDefined();
  }
});
