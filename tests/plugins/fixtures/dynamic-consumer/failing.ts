import { defineFacet } from "@tembra/sdk";

import { DynamicConsumer } from "../../../../target/generated/contracts/dynamic-consumer";

export default defineFacet({
  name: "dynamic-consumer",
  uses: [DynamicConsumer],
  setup() {
    throw new Error("Candidate setup failed before reload adoption");
  },
});
