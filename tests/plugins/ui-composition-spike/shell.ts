import { defineFacet, Type } from "@tembra/sdk";

import { Shell, Surfaces } from "./contracts";

/** The shell owns extension surfaces without importing the feature implementation. */
export default defineFacet({
  name: "spike.shell",
  uses: [Shell],
  setup(_scope, declaration) {
    declaration.serve(
      Shell,
      (_input, self) => {
        self.register(Surfaces, "workspace.screen", { cardinality: "one" });
        self.register(Surfaces, "workspace.actions", { cardinality: "many" });
        const mounted = self.cell("mounted", { schema: Type.Boolean(), initial: true });
        return { state: { mounted }, methods: {} };
      },
      { lifetime: "host" },
    );
  },
});
