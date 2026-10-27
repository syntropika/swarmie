import { useSource } from "@swarmie/client-ui";
import type { ConversationGroupProps } from "@swarmie/plugin-conversations-browser";
import { ChromeButton, Icon, NavigationGroup } from "@swarmie/ui-web";
import { useState } from "react";

import { useProjects } from "./context";

export function ProjectsSidebar({ conversations, renderConversations }: ConversationGroupProps) {
  const { api, open } = useProjects();
  const state = useSource(api.state);
  const [folded, setFolded] = useState<string[]>([]);
  function toggle(id: string) {
    setFolded((current) =>
      current.includes(id) ? current.filter((entry) => entry !== id) : [...current, id],
    );
  }
  return (
    <div className="mb-5">
      <NavigationGroup
        title="Projects"
        expanded={!folded.includes("all")}
        onExpandedChange={() => {
          toggle("all");
        }}
        actions={
          <>
            <ChromeButton
              aria-label="All projects"
              title="All projects"
              onPress={() => {
                open();
              }}
            >
              <Icon name="dots" />
            </ChromeButton>
            <ChromeButton
              aria-label="Create project"
              title="Create project"
              onPress={api.showCreate}
            >
              <Icon name="plus" />
            </ChromeButton>
          </>
        }
      >
        {state.records
          .filter((entry) => !entry.archived)
          .map((project) => (
            <div key={project.id} className="mb-1">
              <div className="flex items-center">
                <ChromeButton
                  aria-label={`Toggle ${project.name} conversations`}
                  aria-expanded={!folded.includes(project.id)}
                  aria-controls={`project-conversations-${project.id}`}
                  onPress={() => {
                    toggle(project.id);
                  }}
                >
                  <Icon
                    name="chevron"
                    className={folded.includes(project.id) ? "-rotate-90" : ""}
                  />
                </ChromeButton>
                <div className="min-w-0 flex-1">
                  <ChromeButton
                    size="navigation"
                    title={project.name}
                    onPress={() => {
                      open(project.id);
                    }}
                  >
                    <Icon name="project" />
                    <span className="truncate">{project.name}</span>
                  </ChromeButton>
                </div>
              </div>
              <div
                id={`project-conversations-${project.id}`}
                hidden={folded.includes(project.id)}
                className="pl-3"
              >
                {renderConversations(
                  conversations.filter(
                    (chat) => !chat.pinned && state.links[chat.id] === project.id,
                  ),
                )}
              </div>
            </div>
          ))}
        {state.records.length === 0 ? (
          <p className="px-2 py-2 text-navigation-label text-muted">No projects yet.</p>
        ) : null}
      </NavigationGroup>
    </div>
  );
}
