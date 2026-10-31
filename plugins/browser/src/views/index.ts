import { updatePresentation } from "@swarmie/client-ui";
import { orb } from "@syntropika/tembra-sdk";

export type ViewValue =
  | null
  | boolean
  | number
  | string
  | readonly ViewValue[]
  | { readonly [key: string]: ViewValue };

/** Serializable capability descriptors preserve the target captured when a tab opens. */
export type CapabilityView = {
  owner: string;
  view: string;
  target: Readonly<Record<string, ViewValue>>;
};

export type BuiltinTab = {
  id: string;
  title: string;
} & (
  | { kind: "start" | "notes" | "state" }
  | { kind: "conversation"; conversationId: string; temporary?: true }
);
export type CapabilityTab = CapabilityView & { id: string; kind: "capability"; title: string };
export type Tab = BuiltinTab | CapabilityTab;
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
