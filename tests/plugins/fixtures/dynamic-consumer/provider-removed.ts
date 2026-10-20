import { defineFacet } from "@tembra/sdk";

/** Capability removal is distinct from an incompatible replacement contract. */
export default defineFacet({
  name: "dynamic-consumer",
  uses: [],
  setup() {
    return undefined;
  },
});
