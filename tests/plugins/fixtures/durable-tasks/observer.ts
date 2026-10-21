import { Type, defineFacet, hookHandler, local, registerHook } from "@tembra/sdk";

import { TaskLifecycle } from "../../../../plugins/durable-tasks/typescript";
import { TaskGate } from "../../../../target/generated/contracts/durable-gate";
import { MemoryStore } from "../../../../target/generated/contracts/memory-store";

const Observer = local("fixture.durable-observer", {
  version: 1,
  input: Type.Object({}),
  members: { observe: TaskLifecycle.method },
});

/** Actual portable series contribution; the memory entries are test assertions only. */
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
          registerHook(self, TaskLifecycle, "durable-observer", "observe", { priority: -5 });
          return {
            state: {},
            methods: {
              observe: hookHandler(TaskLifecycle, async (context, { input: event }) => {
                const key = `${event.task.task_id}:${event.task.revision}:${event.type}`;
                if (
                  version === 1 &&
                  event.task.task_id === "observer-gated" &&
                  event.type === "committed" &&
                  event.origin === "creation"
                ) {
                  await gate.await_gate(context);
                }
                await memory.put(
                  {
                    namespace: "lifecycle-acceptance",
                    key,
                    value: { version, event },
                  },
                  context,
                );
              }),
            },
          };
        },
        { lifetime: "host" },
      );
    },
  });
}
