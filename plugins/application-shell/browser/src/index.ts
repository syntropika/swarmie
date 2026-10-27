import { updatePresentation } from "@swarmie/client-ui";
import { orb } from "@syntropika/tembra-sdk";

import { createShellUi } from "./extensions";

export type { ShellUiApi, SectionProps } from "./extensions";

export type Destination = {
  active: string;
  section: string;
  params: Record<string, string>;
  settings: "general" | "connections" | "plugins" | "providers";
};
export type ShellState = Destination & {
  navigation: boolean;
  summary: boolean;
  history: { entries: Destination[]; index: number };
};
export function applicationShellOrb(wide: boolean) {
  const initial: Destination = {
    active: "",
    section: "conversations",
    settings: "plugins",
    params: {},
  };
  return orb({
    name: "swarmie.shell.ui",
    apiVersion: 1,
    services: {
      "swarmie.shell.ui": {
        create: (self) => {
          self.resource("ui", {
            opaque: true,
            open: createShellUi,
            close: (api) => {
              api.dispose();
            },
          });
          return self.state<ShellState>({
            ...initial,
            navigation: wide,
            summary: false,
            history: { entries: [initial], index: 0 },
          });
        },
        methods: { update: updatePresentation<ShellState> },
      },
    },
  });
}

export { applicationCommandsOrb } from "./commands";

export { createNavigation } from "./navigation";
