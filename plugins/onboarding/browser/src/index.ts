import { updatePresentation } from "@swarmie/client-ui";
import { orb } from "@syntropika/tembra-sdk";

export type OnboardingState = { completed: boolean; open: boolean };
export const onboardingOrb = orb({
  name: "swarmie.onboarding.ui",
  apiVersion: 1,
  services: {
    "swarmie.onboarding.ui": {
      create: (self) => self.state<OnboardingState>({ completed: false, open: false }),
      methods: { update: updatePresentation<OnboardingState> },
    },
  },
});
