import { defineFacet, Type } from "@tembra/sdk";

import { DynamicConsumer } from "../../../target/generated/contracts/dynamic-consumer";
import { serveGenerated } from "./support/generated-provider";

export const first = defineFacet({
  name: "partial-first",
  uses: [DynamicConsumer],
  setup(_scope, declaration) {
    serveGenerated(declaration, DynamicConsumer, (_input, self) => ({
      state: {
        invocations: self.cell("invocations", {
          schema: Type.Integer({ minimum: 0, maximum: 4_294_967_295 }),
          initial: 0,
        }),
      },
      methods: { run: () => 42 },
    }));
  },
});

export const second = defineFacet({
  name: "partial-second",
  uses: [],
  setup() {
    throw new Error("Second initial facet setup failed after the first installation");
  },
});
