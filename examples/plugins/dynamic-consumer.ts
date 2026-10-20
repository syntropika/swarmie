import { defineFacet, Type } from "@tembra/sdk";

import { serveGenerated } from "./generated-provider";
import { Counter } from "./generated/counter";
import { DynamicConsumer } from "./generated/dynamic-consumer";

/** Uses native Counter directly; administration does not participate in invocation. */
export function consumer(step: number) {
  return defineFacet({
    name: "dynamic-consumer",
    uses: [Counter, DynamicConsumer],
    setup(scope, declaration) {
      const capabilities = scope.registry({ name: "swarmie.capabilities", conflict: "error" });
      serveGenerated(declaration, DynamicConsumer, (_input, self) => {
        const invocations = self.cell("invocations", {
          schema: Type.Integer({ minimum: 0, maximum: 4_294_967_295 }),
          initial: 0,
        });
        return {
          state: { invocations },
          methods: {
            async run(context) {
              if (capabilities.table(context).get("swarmie.counter") === undefined) {
                throw new Error("Counter capability is unavailable on this node");
              }
              const counter = await context.get(Counter, {});
              try {
                const result = await counter.increment(step, context);
                invocations.replace(context, invocations.value + 1);
                return result;
              } finally {
                counter.release();
              }
            },
          },
        };
      });
    },
  });
}
