import { defineFacet } from "@tembra/sdk";

import { DynamicConsumer } from "../../../../target/generated/contracts/dynamic-consumer";
import { serveGenerated } from "../support/generated-provider";

/** Compatible metadata reaches the real candidate factory, which refuses before adoption. */
export default defineFacet({
  name: "counter-provider-failing.runtime",
  uses: [DynamicConsumer],
  setup(_scope, declaration) {
    serveGenerated(
      declaration,
      DynamicConsumer,
      () => {
        throw new Error("injected_replacement_provider_factory_failure");
      },
      { lifetime: "host" },
    );
  },
});
