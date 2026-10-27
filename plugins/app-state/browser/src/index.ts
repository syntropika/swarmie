import { orb } from "@syntropika/tembra-sdk";
/** The application installs this optional inspector only in development. */
export const appStateOrb = orb({
  name: "swarmie.app-state.ui",
  apiVersion: 1,
  services: {
    "swarmie.app-state.ui": { create: (self) => self.state({ available: true }), methods: {} },
  },
});
