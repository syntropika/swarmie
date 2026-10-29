import { updatePresentation } from "@swarmie/client-ui";
import { orb } from "@syntropika/tembra-sdk";

export type Tab = {
  id: string;
  title: string;
} & (
  | { kind: "start" | "notes" | "state" }
  | { kind: "conversation"; conversationId: string; temporary?: true }
);
export type ViewsState = {
  tabs: Tab[];
  tab: string;
  conversationVisible: boolean;
  conversationFirst: boolean;
  expanded: boolean;
  mobile: "conversation" | "view";
};
export const initialViews: ViewsState = {
  tabs: [],
  tab: "",
  conversationVisible: true,
  conversationFirst: false,
  expanded: false,
  mobile: "conversation",
};
export const viewsOrb = orb({
  name: "swarmie.views.ui",
  apiVersion: 1,
  services: {
    "swarmie.views.ui": {
      create: (self) => self.state(initialViews),
      methods: { update: updatePresentation<ViewsState> },
    },
  },
});

export { createViewsApi } from "./commands";
