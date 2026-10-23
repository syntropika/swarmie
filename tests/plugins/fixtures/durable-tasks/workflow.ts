import { defineFacet } from "@tembra/sdk";

import type { TaskTransition } from "../../../../plugins/durable-tasks/typescript";
import { defineTask } from "../../../../plugins/durable-tasks/typescript";
import { TaskGate } from "../../../../target/generated/contracts/durable-gate";
import { DurableWorkflow } from "../../../../target/generated/contracts/durable-workflow";
import { serveGenerated } from "../support/generated-provider";

/** Same application Contract and Registry identity across actual compatible JS generations. */
export function workflow(version: number) {
  return defineFacet({
    name: "fixture.durable-workflow",
    uses: [DurableWorkflow, TaskGate],
    setup(_scope, declaration) {
      serveGenerated(
        declaration,
        DurableWorkflow,
        (input, self) => {
          const gate = self.get(TaskGate, {});
          return defineTask({
            kind: "fixture.workflow",
            version,
            async step(context, invocation) {
              const transition = async (): Promise<TaskTransition> => {
                const { task } = invocation;
                if (invocation.type === "migrate") {
                  return task.input !== null &&
                    typeof task.input === "object" &&
                    !Array.isArray(task.input) &&
                    Reflect.get(task.input, "reject_migration") === true
                    ? { type: "reject_migration", reason: "unsupported checkpoint" }
                    : { type: "migrate", checkpoint: task.checkpoint, phase: task.phase };
                }
                if (task.phase === "prepare") {
                  const wakeAt: unknown =
                    task.input !== null &&
                    typeof task.input === "object" &&
                    !Array.isArray(task.input)
                      ? Reflect.get(task.input, "wake_at_ms")
                      : undefined;
                  return {
                    type: "sleep",
                    checkpoint: { operation_id: `${task.id}:publish`, prepared_by: version },
                    phase: "finish",
                    wake_at_ms: typeof wakeAt === "number" ? wakeAt : 1,
                  };
                }
                if (task.phase === "gated") {
                  await gate.await_gate(context);
                  return {
                    type: "checkpoint",
                    checkpoint: { executed_by: version },
                    phase: "finish",
                  };
                }
                if (task.phase === "gated-continue") {
                  await gate.await_gate(context);
                  return {
                    type: "continue",
                    checkpoint: { executed_by: version },
                    phase: "finish",
                  };
                }
                return { type: "complete", value: { published: true, version } };
              };
              return { transition: await transition(), session_appends: [] };
            },
          })(input, self);
        },
        { lifetime: "host" },
      );
    },
  });
}
