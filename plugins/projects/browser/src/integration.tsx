import type { applicationShellOrb, ShellUiApi } from "@swarmie/plugin-browser/shell";
import { conversationsOrb } from "@swarmie/plugin-conversations-browser";
import type { ConversationsUiApi } from "@swarmie/plugin-conversations-browser";
import { orb } from "@syntropika/tembra-sdk";
import { Folders } from "lucide-react";

import type { projectsOrb, ProjectsApi } from "./index";
import { ProjectSelector, ProjectConversationLabel } from "./selector";
import { ProjectDialog, ProjectsPage, ProjectsHeader, ProjectsSidebar } from "./views";

/** Projects owns these optional integrations. Its state Orb has no Conversations requirement. */
export function projectsIntegrationOrb(
  api: ProjectsApi,
  services: {
    projects: ReturnType<typeof projectsOrb>["services"]["swarmie.projects.ui"];
    shell: ReturnType<typeof applicationShellOrb>["services"]["swarmie.shell.ui"];
  },
) {
  return orb({
    name: "swarmie.projects.integration",
    apiVersion: 1,
    requires: {
      projects: {
        orb: "swarmie.projects.ui",
        apiVersion: 1,
        services: { "swarmie.projects.ui": [] },
      },
      shell: { orb: "swarmie.shell.ui", apiVersion: 1, services: { "swarmie.shell.ui": [] } },
      conversations: {
        orb: "swarmie.conversations.ui",
        apiVersion: 1,
        services: { "swarmie.conversations.ui": [] },
      },
    },
    services: {
      "swarmie.projects.integration": {
        create(self) {
          const project = self.get(services.projects, "main");
          const shell = self.get(services.shell, "main");
          const conversations = self.get(
            conversationsOrb.services["swarmie.conversations.ui"],
            "main",
          );
          self.resource("contributions", {
            deps: [project, shell, conversations],
            open() {
              const shellUi = shell.opaque<ShellUiApi>("ui");
              const ui = conversations.opaque<ConversationsUiApi>("ui");
              const dispose = [
                ui.lifecycle.registerCreated({
                  id: "swarmie.projects.admit",
                  owner: "projects",
                  execute: (conversationId) => {
                    api.admitDraft(conversationId).catch(api.fail);
                  },
                }),
                shellUi.registerSection({
                  id: "projects",
                  owner: "projects",
                  label: "Projects",
                  icon: Folders,
                  component: ProjectsPage,
                  header: ProjectsHeader,
                  overlay: ProjectDialog,
                }),
                ui.composer.topBar.registerItem({
                  id: "swarmie.projects.selector",
                  owner: "projects",
                  side: "left",
                  priority: 50,
                  component: ProjectSelector,
                }),
                ui.navigation.registerGroup({
                  id: "swarmie.projects.group",
                  owner: "projects",
                  component: ProjectsSidebar,
                  membership: api.membership,
                }),
                ui.header.registerItem({
                  id: "swarmie.projects.label",
                  owner: "projects",
                  component: ProjectConversationLabel,
                }),
              ];
              return () => {
                for (const close of dispose) {
                  close();
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
