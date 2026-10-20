import { defineFacet } from "@tembra/sdk";

export default defineFacet({
  name: "browser-widget",
  uses: [],
  setup() {
    throw new Error("Browser candidate setup refused");
  },
});
