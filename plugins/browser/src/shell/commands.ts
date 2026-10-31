import { conversationsOrb } from "@swarmie/plugin-conversations-browser";
import type { ConversationsUiApi } from "@swarmie/plugin-conversations-browser";
import { orb } from "@syntropika/tembra-sdk";
import { Globe, Pin, Square } from "lucide-react";

export function applicationCommandsOrb(actions: {
  openView: () => void;
  showContext: () => void;
  stop: () => void;
}) {
  return orb({
    name: "swarmie.shell.commands",
    apiVersion: 1,
    requires: {
      conversations: {
        orb: "swarmie.conversations.ui",
        apiVersion: 1,
        services: { "swarmie.conversations.ui": [] },
      },
    },
    services: {
      "swarmie.shell.commands": {
        create(self) {
          const owner = self.get(conversationsOrb.services["swarmie.conversations.ui"], "main");
          self.resource("commands", {
            deps: [owner],
            open: () => {
              const api = owner.opaque<ConversationsUiApi>("ui");
              const entries = [
                {
                  id: "swarmie.shell.view",
                  command: "view",
                  label: "Open a view",
                  icon: Globe,
                  execute: actions.openView,
                },
                {
                  id: "swarmie.shell.context",
                  command: "context",
                  label: "Show pinned context",
                  icon: Pin,
                  execute: actions.showContext,
                },
                {
                  id: "swarmie.shell.stop",
                  command: "stop",
                  label: "Stop this response",
                  icon: Square,
                  execute: actions.stop,
                },
              ];
              const disposers = entries.map((entry) =>
                api.composer.registerSlashCommand({ ...entry, owner: "swarmie.shell" }),
              );
              return () => {
                for (const dispose of disposers) {
                  dispose();
                }
              };
            },
            close: (dispose) => {
              dispose();
            },
          });
          return {};
        },
        methods: {},
      },
    },
  });
}
