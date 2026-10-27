import { useSource } from "@swarmie/client-ui";
import type { ConversationGroupProps } from "@swarmie/plugin-conversations-browser";
import { ChromeButton } from "@swarmie/ui-web";
import { ChevronDown, Folder, MoreHorizontal, Plus } from "lucide-react";
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
    <section className="mb-5">
      <div className="mb-1 flex h-7 items-center gap-1 px-1">
        <button
          type="button"
          aria-expanded={!folded.includes("all")}
          onClick={() => {
            toggle("all");
          }}
          className="flex flex-1 items-center gap-1 text-left text-chrome text-muted hover:text-foreground"
        >
          Projects
          <ChevronDown className={`size-3 ${folded.includes("all") ? "-rotate-90" : ""}`} />
        </button>
        <ChromeButton
          size="application"
          aria-label="All projects"
          title="All projects"
          onPress={() => {
            open();
          }}
        >
          <MoreHorizontal className="size-3.5" />
        </ChromeButton>
        <ChromeButton
          size="application"
          aria-label="Create project"
          title="Create project"
          onPress={api.showCreate}
        >
          <Plus className="size-3.5" />
        </ChromeButton>
      </div>
      {folded.includes("all")
        ? null
        : state.records
            .filter((entry) => !entry.archived)
            .map((project) => (
              <div key={project.id} className="mb-1">
                <div className="flex items-center">
                  <button
                    type="button"
                    aria-label={`Toggle ${project.name} conversations`}
                    aria-expanded={!folded.includes(project.id)}
                    onClick={() => {
                      toggle(project.id);
                    }}
                    className="p-1 text-muted hover:text-foreground"
                  >
                    <ChevronDown
                      className={`size-3 ${folded.includes(project.id) ? "-rotate-90" : ""}`}
                    />
                  </button>
                  <button
                    type="button"
                    onClick={() => {
                      open(project.id);
                    }}
                    className="flex h-8 min-w-0 flex-1 items-center gap-2 rounded-md px-1 text-left text-xs hover:bg-row-hover"
                  >
                    <Folder className="size-3.5 shrink-0 text-muted" />
                    <span className="truncate">{project.name}</span>
                  </button>
                </div>
                {folded.includes(project.id) ? null : (
                  <div className="pl-3">
                    {renderConversations(
                      conversations.filter(
                        (chat) => !chat.pinned && state.links[chat.id] === project.id,
                      ),
                    )}
                  </div>
                )}
              </div>
            ))}
      {!folded.includes("all") && state.records.length === 0 ? (
        <p className="px-2 py-2 text-chrome text-muted">No projects yet.</p>
      ) : null}
    </section>
  );
}
