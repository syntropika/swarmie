import { updatePresentation } from "@swarmie/client-ui";
import { orb } from "@syntropika/tembra-sdk";

import { initialPresentation } from "./types";
import type { Presentation } from "./types";

export { createContextPresentation } from "./presentation";
export type * from "./types";
export const contextUiOrb = orb({
  name: "swarmie.context.ui",
  apiVersion: 1,
  services: {
    "swarmie.context.ui": {
      create: (self) => self.state<Presentation>(initialPresentation),
      methods: { update: updatePresentation<Presentation> },
    },
  },
});
