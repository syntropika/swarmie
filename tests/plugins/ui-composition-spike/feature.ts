import { defineFacet } from "@tembra/sdk";

import { RemoteCounter } from "../../../target/generated/contracts/remote-counter";
import { Contributions, Counter, Lifetime, Snapshot } from "./contracts";

/** A host-owned Browser Facet contributes both a view and a checked operation. */
export function feature(generation: number) {
  return defineFacet({
    name: "browser-widget",
    uses: [Counter, RemoteCounter],
    setup(_scope, declaration) {
      declaration.serve(
        Counter,
        (_input, self) => {
          const remote = self.get(RemoteCounter, {});
          const snapshot = self.cell("snapshot", {
            schema: Snapshot,
            initial: { count: 0, generation: 1, serverResult: null },
          });
          self.resource("lifetime", {
            open() {
              Lifetime.opened += 1;
              return {};
            },
            close() {
              Lifetime.closed += 1;
            },
          });
          self.register(Contributions, "counter.screen", {
            kind: "screen",
            surface: "workspace.screen",
            label: `Counter screen ${generation}`,
            module: "views.js",
            export: "CounterScreen",
            generation,
          });
          self.registerMethod(
            Contributions,
            "counter.increment",
            {
              kind: "action",
              surface: "workspace.actions",
              label: `Increment by ${generation}`,
              generation,
            },
            "increment",
          );
          return {
            state: { snapshot },
            methods: {
              async increment(context) {
                const serverResult = await remote.increment(generation, context);
                const next = { count: snapshot.value.count + generation, generation, serverResult };
                snapshot.replace(context, next);
                return next;
              },
            },
          };
        },
        { lifetime: "host" },
      );
    },
  });
}
