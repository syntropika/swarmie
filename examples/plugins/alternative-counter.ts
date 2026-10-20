import { defineFacet, Type } from "@tembra/sdk";

import { serveGenerated } from "./generated-provider";
import { Counter } from "./generated/counter";
import { PluginConfiguration } from "./generated/plugin-configuration";
import { RemoteCounter } from "./generated/remote-counter";

/** An independently distributed implementation of the canonical Counter contracts. */
export default defineFacet({
  name: "alternative-counter",
  uses: [Counter, RemoteCounter, PluginConfiguration],
  setup(_scope, declaration) {
    serveGenerated(
      declaration,
      Counter,
      (_input, self) => {
        const configuration = self.get(PluginConfiguration, { plugin_id: "alternative-counter" });
        const count = self.cell("count", {
          schema: Type.Integer({ minimum: 0, maximum: 4_294_967_295 }),
          async initialize(context) {
            const value = await configuration.read(context);
            if (
              value === null ||
              typeof value !== "object" ||
              Array.isArray(value) ||
              Object.keys(value).some((key) => key !== "initial")
            ) {
              throw new Error("Alternative Counter configuration must contain only initial");
            }
            const initial: unknown = Reflect.get(value, "initial");
            if (initial === undefined) {
              return 10;
            }
            if (
              typeof initial !== "number" ||
              !Number.isInteger(initial) ||
              initial < 0 ||
              initial > 4_294_967_295
            ) {
              throw new Error("Alternative Counter initial must be an unsigned 32-bit integer");
            }
            return initial;
          },
        });
        self.registerMethod(
          { name: "swarmie.capabilities", conflict: "error" },
          "swarmie.counter",
          { plugin_id: "alternative-counter", contract: "swarmie.counter", operation: "increment" },
          "increment",
        );
        return {
          state: { count },
          methods: {
            increment(context, by) {
              const next = count.value + by;
              if (next > 4_294_967_295) {
                throw new Error("counter_overflow");
              }
              count.replace(context, next);
              return next;
            },
            read() {
              return count.value;
            },
          },
        };
      },
      { lifetime: "host" },
    );
    serveGenerated(declaration, RemoteCounter, (_input, self) => {
      const counter = self.get(Counter, {});
      return {
        variant: "main",
        state: { count: counter.count },
        methods: {
          increment: async (context, by) => await counter.increment(by, context),
          read: async (context) => await counter.read(context),
        },
      };
    });
  },
});
