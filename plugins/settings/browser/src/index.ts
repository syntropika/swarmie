import { updatePresentation } from "@swarmie/client-ui";
import { orb } from "@syntropika/tembra-sdk";

export type PluginSetting = {
  id: string;
  name: string;
  description: string;
  enabled: boolean;
  required: boolean;
  errors: string[];
  uses: string[];
  version: string;
  category: string;
};
export type SettingsState = {
  plugins: PluginSetting[];
  theme: "system" | "dark" | "light";
  language: string;
};
export const settingsOrb = orb({
  name: "swarmie.settings.ui",
  apiVersion: 1,
  services: {
    "swarmie.settings.ui": {
      create: (self) =>
        self.state<SettingsState>({ plugins: [], theme: "system", language: "en-US" }),
      methods: { update: updatePresentation<SettingsState> },
    },
  },
});
