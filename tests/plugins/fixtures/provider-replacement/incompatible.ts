import { defineFacet, Type } from "@tembra/sdk";

import { DynamicConsumerV2 } from "../../../../target/generated/contracts/dynamic-consumer-v2";
import { serveGenerated } from "../support/generated-provider";

/** The existing Rust-authored version 2 refuses held version 1 consumer requirements. */
export default defineFacet({
  name: "counter-provider-incompatible.runtime",
  uses: [DynamicConsumerV2],
  setup(_scope, declaration) {
    serveGenerated(
      declaration,
      DynamicConsumerV2,
      (_input, self) => ({
        state: {
          invocations: self.cell("invocations", {
            schema: Type.Integer({ minimum: 0, maximum: 4_294_967_295 }),
            initial: 0,
          }),
        },
        methods: { run: () => "incompatible replacement result" },
      }),
      { lifetime: "host" },
    );
  },
});
