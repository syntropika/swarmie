import { useSource } from "@swarmie/client-ui";
import type { ConversationControlProps } from "@swarmie/plugin-conversations-browser";
import { ChromeButton, Popover } from "@swarmie/ui-web";
import { Check, Folder, MessageCircle, Plus } from "lucide-react";
import { useState } from "react";

import { useProjects } from "./context";

export function ProjectSelector({ conversationId, isDraft }: ConversationControlProps) {
  const { api } = useProjects();
  const state = useSource(api.state);
  const [open, setOpen] = useState(false);
  const selected = (isDraft ? state.draftLinks : state.links)[conversationId];
  const project = state.records.find((entry) => entry.id === selected);
  async function choose(projectId: string | null) {
    try {
      if (isDraft) {
        await api.stageConversation(conversationId, projectId);
      } else {
        await (projectId === null
          ? api.unassignConversation(conversationId)
          : api.assignConversation({ conversationId, projectId }));
      }
      setOpen(false);
    } catch (error: unknown) {
      api.fail(error);
    }
  }
  return (
    <Popover isOpen={open} onOpenChange={setOpen}>
      <ChromeButton
        size="composerCap"
        aria-label="Select project"
        title={project?.name ?? "No project"}
        selected={open}
      >
        <Folder className="size-3 shrink-0" />
        <span className="max-w-32 truncate">
          {project?.name ?? (selected === undefined ? "No project" : "Unavailable project")}
        </span>
      </ChromeButton>
      <Popover.Content placement="top start" kind="composer" className="z-50">
        <Popover.Dialog aria-label="Choose a project">
          <p className="px-2 py-1 text-chrome text-muted">Project</p>
          <button
            type="button"
            onClick={() => {
              choose(null).catch(api.fail);
            }}
            className="flex min-h-8 w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs hover:bg-row-hover"
          >
            <span className="flex-1">No project</span>
            {selected === undefined ? <Check className="size-3 text-brand-accent" /> : null}
          </button>
          {state.records
            .filter((entry) => !entry.archived)
            .map((entry) => (
              <button
                key={entry.id}
                type="button"
                onClick={() => {
                  choose(entry.id).catch(api.fail);
                }}
                className="flex min-h-8 w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs hover:bg-row-hover"
              >
                <Folder className="size-3 text-muted" />
                <span className="min-w-0 flex-1 truncate">{entry.name}</span>
                {entry.id === selected ? <Check className="size-3 text-brand-accent" /> : null}
              </button>
            ))}
          <div className="my-1 border-t border-border" />
          <ChromeButton
            size="action"
            onPress={() => {
              setOpen(false);
              api.showCreate();
            }}
          >
            <Plus className="size-3" />
            Create project
          </ChromeButton>
        </Popover.Dialog>
      </Popover.Content>
    </Popover>
  );
}

export function ProjectConversationLabel({ conversationId, isDraft }: ConversationControlProps) {
  const { api, open } = useProjects();
  const state = useSource(api.state);
  const id = (isDraft ? state.draftLinks : state.links)[conversationId];
  if (id === undefined) {
    return (
      <>
        <MessageCircle className="size-3.5 shrink-0 text-muted" />
        <span className="hidden text-muted sm:inline">Chat</span>
        <span className="hidden text-muted sm:inline">/</span>
      </>
    );
  }
  const project = state.records.find((entry) => entry.id === id);
  return (
    <>
      <Folder className="size-3.5 shrink-0 text-muted" />
      <button
        type="button"
        className="hidden max-w-40 truncate text-muted hover:text-foreground sm:inline"
        onClick={() => {
          open(id);
        }}
      >
        {project?.name ?? "Unavailable project"}
      </button>
      <span className="hidden text-muted sm:inline">/</span>
    </>
  );
}
