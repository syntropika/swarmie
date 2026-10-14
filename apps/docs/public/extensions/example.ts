import type { ClientExtension } from "@swarmie/contracts/ui-client";

// Register this bundled module with the workspace's ContributionRegistry.
export const example: ClientExtension = {
  id: "example",
  manifest: {
    version: 1,
    commands: [{ id: "insert", title: "Insert example", when: ["composerReady"], shortcut: "Mod+Shift+i" }],
    slots: [{
      id: "insert", slot: "composer.actions", kind: "action", title: "Insert example",
      command: "insert", order: 100, enabledWhen: ["composerReady"],
    }],
    screens: [],
    panels: [],
  },
  handlers: { insert: (api) => api.insertText("Example text from the contribution demo. ") },
};
