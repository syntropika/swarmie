import { useSource } from "@swarmie/client-ui";
import type { ConversationControlProps } from "@swarmie/plugin-conversations-browser";
import { ChromeButton, ListBox, Popover } from "@swarmie/ui-web";
import { Folder, MessageCircle, Plus } from "lucide-react";
import { useState } from "react";

import { useProjects } from "./context";

export function ProjectSelector({ conversationId, isDraft }: ConversationControlProps) {
  const { api } = useProjects();
  const state = useSource(api.state);
  const [open, setOpen] = useState(false);
  const selected = (isDraft ? state.draftLinks : state.links)[conversationId];
  const project = state.records.find((entry) => entry.id === selected);
  const projectLabel =
    project?.name ?? (selected === undefined ? "No project" : "Unavailable project");
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
        aria-label={`${projectLabel}: Select project`}
        title={projectLabel}
        selected={open}
      >
        <Folder className="size-3 shrink-0" />
        <span className="max-w-32 truncate">{projectLabel}</span>
      </ChromeButton>
      <Popover.Content placement="top start" kind="composer" className="z-50">
        <Popover.Dialog aria-label="Choose a project">
          <p className="px-2 py-1 text-chrome text-muted">Project</p>
          <ListBox
            aria-label="Project"
            selectionMode="single"
            selectionBehavior="toggle"
            disallowEmptySelection
            selectedKeys={[selected === undefined ? "unassigned" : `project:${selected}`]}
            kind="composer"
          >
            <ListBox.Item
              id="unassigned"
              textValue="No project"
              aria-label="No project"
              kind="composer"
              onPress={() => {
                choose(null).catch(api.fail);
              }}
            >
              <span className="flex-1">No project</span>
              <ListBox.ItemIndicator />
            </ListBox.Item>
            {state.records
              .filter((entry) => !entry.archived)
              .map((entry) => (
                <ListBox.Item
                  key={entry.id}
                  id={`project:${entry.id}`}
                  textValue={entry.name}
                  aria-label={entry.name}
                  kind="composer"
                  onPress={() => {
                    choose(entry.id).catch(api.fail);
                  }}
                >
                  <Folder className="size-3 text-muted" />
                  <span className="min-w-0 flex-1 truncate">{entry.name}</span>
                  <ListBox.ItemIndicator />
                </ListBox.Item>
              ))}
          </ListBox>
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
