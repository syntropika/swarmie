import { SwarmieClient } from "@swarmie/api";

import { createAuthEffects } from "./identity/authentication/effects";
import { createSessionQueryOptions } from "./identity/session/query";

export const authEffects = createAuthEffects(
  new SwarmieClient({
    fetch: () => {
      throw new Error("Unexpected test transport");
    },
  }),
);
export const sessionQueryOptions = createSessionQueryOptions(authEffects);
