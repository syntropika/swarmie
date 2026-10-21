import { Type, defineFacet, hookHandler, local, registerHook } from "@tembra/sdk";

import { onTaskCreated, onTaskTerminal } from "../../../../plugins/durable-tasks/typescript";
import type { TaskCreated } from "../../../../plugins/durable-tasks/typescript";
import { TaskGate } from "../../../../target/generated/contracts/durable-gate";
import { MemoryStore } from "../../../../target/generated/contracts/memory-store";

const Observer = local("fixture.durable-observer", {
  version: 1,
  input: Type.Object({}),
  members: { created: onTaskCreated.method, terminal: onTaskTerminal.method },
});

function record(version: number, transition: TaskCreated["transition"]) {
  return {
    namespace: "lifecycle-acceptance",
    key: `${transition.task.task_id}:${transition.task.revision}:committed`,
    value: { version, event: { type: "committed", ...transition } },
  };
}

/** The plugin subscribes to two independent typed hooks and filters no lifecycle event kinds. */
export function observer(version: number) {
  return defineFacet({
    name: "fixture.durable-observer",
    uses: [MemoryStore, TaskGate],
    setup(_scope, declaration) {
      declaration.serve(
        Observer,
        (_input, self) => {
          const memory = self.get(MemoryStore, {});
          const gate = self.get(TaskGate, {});
          registerHook(self, onTaskCreated, "durable-observer-created", "created", {
            priority: -5,
          });
          registerHook(self, onTaskTerminal, "durable-observer-terminal", "terminal", {
            priority: -5,
          });
          return {
            state: {},
            methods: {
              created: hookHandler(onTaskCreated, async (context, { input }) => {
                if (version === 1 && input.transition.task.task_id === "observer-gated") {
                  await gate.await_gate(context);
                }
                await memory.put(record(version, input.transition), context);
              }),
              terminal: hookHandler(onTaskTerminal, async (context, { input }) => {
                await memory.put(record(version, input.transition), context);
              }),
            },
          };
        },
        { lifetime: "host" },
      );
    },
  });
}
