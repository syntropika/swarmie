import { defineFacet, Type } from "@tembra/sdk";

import { serveGenerated } from "./generated-provider";
import { DynamicConsumerV2 } from "./generated/dynamic-consumer-v2";

/** The Rust-authored version 2 changes the result type required by existing version 1 consumers. */
export default defineFacet({
  name: "dynamic-consumer",
  uses: [DynamicConsumerV2],
  setup(_scope, declaration) {
    serveGenerated(declaration, DynamicConsumerV2, (_input, self) => ({
      state: {
        invocations: self.cell("invocations", {
          schema: Type.Integer({ minimum: 0, maximum: 4_294_967_295 }),
          initial: 0,
        }),
      },
      methods: { run: () => "incompatible result" },
    }));
  },
});
