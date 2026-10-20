import { defineFacet } from "@tembra/sdk";

/** Browser-target artifact for native administration receipt tests; it has no renderer. */
export default defineFacet({
  name: "browser-widget",
  uses: [],
  setup() {
    // Receipt qualification requires an artifact, with no rendered contributions.
  },
});
