import { defineFacet, Type } from "@tembra/sdk";

import { Counter } from "../../../../target/generated/contracts/counter";
import { DynamicConsumer } from "../../../../target/generated/contracts/dynamic-consumer";
import { serveGenerated } from "../support/generated-provider";

/** Actual dependency readiness lets the embedding observe or interrupt candidate initialization. */
export default defineFacet({
  name: "counter-provider-gated.runtime",
  uses: [Counter, DynamicConsumer],
  setup(_scope, declaration) {
    serveGenerated(
      declaration,
      DynamicConsumer,
      (_input, self) => {
        const counter = self.get(Counter, {});
        const ready = self.cell("candidate-ready-gate", {
          schema: Type.Boolean(),
          async initialize(context) {
            await counter.read(context);
            return true;
          },
        });
        const invocations = self.cell("invocations", {
          schema: Type.Integer({ minimum: 0, maximum: 4_294_967_295 }),
          initial: 0,
        });
        return {
          state: { invocations },
          methods: {
            run(context) {
              if (!ready.value) {
                throw new Error("Provider dependency is not ready");
              }
              const next = invocations.value + 10;
              if (next > 4_294_967_295) {
                throw new Error("replacement_provider_overflow");
              }
              invocations.replace(context, next);
              return next;
            },
          },
        };
      },
      { lifetime: "host" },
    );
  },
});
