import { defineFacet, Type } from "@tembra/sdk";

import { DynamicConsumer } from "../../../../target/generated/contracts/dynamic-consumer";
import { PluginConfiguration } from "../../../../target/generated/contracts/plugin-configuration";
import { serveGenerated } from "../support/generated-provider";

/** Different packages and executable Facets implement the same Rust-authored Contract. */
export function provider(facetId: string, step: number) {
  return defineFacet({
    name: facetId,
    uses: [DynamicConsumer, PluginConfiguration],
    setup(_scope, declaration) {
      serveGenerated(
        declaration,
        DynamicConsumer,
        (_input, self) => {
          const configuration = self.get(PluginConfiguration, { facet_id: facetId });
          const binding = self.cell(`binding-${facetId}`, {
            schema: Type.Boolean(),
            async initialize(context) {
              const input = await configuration.read(context);
              if (
                input.instance_id !== "provider" ||
                input.configuration === null ||
                typeof input.configuration !== "object" ||
                Array.isArray(input.configuration) ||
                Object.keys(input.configuration).length !== 1 ||
                Reflect.get(input.configuration, "marker") !== "shared"
              ) {
                throw new Error("Candidate Facet did not receive the acknowledged instance input");
              }
              return true;
            },
          });
          const invocations = self.cell("invocations", {
            schema: Type.Integer({ minimum: 0, maximum: 4_294_967_295 }),
            initial: 0,
          });
          self.registerMethod(
            { name: "swarmie.capabilities", conflict: "error" },
            "replacement-provider/run",
            { facet_id: facetId, contract: "swarmie.dynamic-consumer" },
            "run",
          );
          return {
            state: { invocations },
            methods: {
              run(context) {
                if (!binding.value) {
                  throw new Error("Provider configuration binding is not ready");
                }
                const next = invocations.value + step;
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
}
