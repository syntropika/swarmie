import { defineFacet, Type } from "@tembra/sdk";

import { DynamicConsumerV2 } from "../../../../target/generated/contracts/dynamic-consumer-v2";
import { serveGenerated } from "../support/generated-provider";

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
