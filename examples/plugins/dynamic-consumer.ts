import { defineFacet, Type } from "@tembra/sdk";

import { serveGenerated } from "./generated-provider";
import { Counter } from "./generated/counter";
import { DynamicConsumer } from "./generated/dynamic-consumer";

/** Uses the declared Counter capability directly; administration does not dispatch calls. */
export function consumer(step: number) {
  return defineFacet({
    name: "dynamic-consumer",
    uses: [Counter, DynamicConsumer],
    setup(_scope, declaration) {
      serveGenerated(
        declaration,
        DynamicConsumer,
        (_input, self) => {
          const counter = self.get(Counter, {});
          self.registerMethod(
            { name: "swarmie.capabilities", conflict: "error" },
            "dynamic-consumer/run",
            {
              plugin_id: "dynamic-consumer",
              contract: "swarmie.dynamic-consumer",
              operation: "run",
            },
            "run",
          );
          const invocations = self.cell("invocations", {
            schema: Type.Integer({ minimum: 0, maximum: 4_294_967_295 }),
            initial: 0,
          });
          return {
            state: { invocations },
            methods: {
              async run(context) {
                const result = await counter.increment(step, context);
                invocations.replace(context, invocations.value + 1);
                return result;
              },
            },
          };
        },
        { lifetime: "host" },
      );
    },
  });
}
