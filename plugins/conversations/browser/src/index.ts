import { updatePresentation } from "@swarmie/client-ui";
import { orb } from "@syntropika/tembra-sdk";
import type { State } from "@syntropika/tembra-sdk";

import { createConversationsUi } from "./extensions";
import type { ConversationsState } from "./types";

export type * from "./types";
export type * from "./extensions";
export { createConversationsUi } from "./extensions";
export const conversationsOrb = orb({
  name: "swarmie.conversations.ui",
  apiVersion: 1,
  services: {
    "swarmie.conversations.ui": {
      create(self) {
        self.resource("ui", {
          opaque: true,
          open: createConversationsUi,
          close: (api) => {
            api.dispose();
          },
        });
        return self.state<ConversationsState>({ records: [], error: null });
      },
      methods: {
        update: updatePresentation<ConversationsState>,
        clearError(state: State<ConversationsState>, context) {
          state.update(context, (value) => ({ ...value, error: null }));
        },
      },
    },
  },
});

export { createConversationRecords, defaultConversationTitle } from "./records";
export type { Conversation } from "./records";
