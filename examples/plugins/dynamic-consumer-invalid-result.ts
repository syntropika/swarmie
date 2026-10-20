import { defineFacet, Type } from "@tembra/sdk";

import { serveGenerated } from "./generated-provider";
import { DynamicConsumer } from "./generated/dynamic-consumer";

/** Negative fixture: the canonical unsigned result schema must refuse a returned negative value. */
export default defineFacet({
  name: "dynamic-consumer",
  uses: [DynamicConsumer],
  setup(_scope, declaration) {
    serveGenerated(declaration, DynamicConsumer, (_input, self) => ({
      state: {
        invocations: self.cell("invocations", {
          schema: Type.Integer({ minimum: 0, maximum: 4_294_967_295 }),
          initial: 0,
        }),
      },
      methods: { run: () => -1 },
    }));
  },
});
