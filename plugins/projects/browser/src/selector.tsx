import { useSource } from "@swarmie/client-ui";
import type { ConversationControlProps } from "@swarmie/plugin-conversations-browser";
import {
  ActionMenuPopover,
  ChromeButton,
  Dropdown,
  Header,
  Label,
  Separator,
} from "@swarmie/ui-web";
import { Folder, MessageCircle, Plus } from "lucide-react";
import { useState } from "react";

import { useProjects } from "./context";

export function ProjectSelector({ conversationId, isDraft }: ConversationControlProps) {
  const { api } = useProjects();
  const state = useSource(api.state);
  const [open, setOpen] = useState(false);
  const selected = (isDraft ? state.draftLinks : state.links)[conversationId];
  const projects = state.records.filter((entry) => !entry.archived);
  const project = projects.find((entry) => entry.id === selected);
  const selectedKey = selected === undefined ? "unassigned" : `project:${selected}`;
  const projectLabel =
    project?.name ?? (selected === undefined ? "No project" : "Unavailable project");
  function choose(projectId: string | null) {
    setOpen(false);
    if (isDraft) {
      api.stageConversation(conversationId, projectId).catch(api.fail);
      return;
    }
    const change =
      projectId === null
        ? api.unassignConversation(conversationId)
        : api.assignConversation({ conversationId, projectId });
    change.catch(api.fail);
  }
  return (
    <Dropdown isOpen={open} onOpenChange={setOpen}>
      <ChromeButton
        size="composerCap"
        aria-label={`${projectLabel}: Select project`}
        title={projectLabel}
        selected={open}
        className="max-w-full"
      >
        <Folder className="shrink-0" />
        <span className="max-w-32 truncate">{projectLabel}</span>
      </ChromeButton>
      <ActionMenuPopover placement="top start" className="w-(--spacing-menu-width)">
        <Dropdown.Menu
          aria-label="Choose a project"
          shouldCloseOnSelect
          onAction={(key) => {
            if (key === "create") {
              setOpen(false);
              api.showCreate();
            } else if (key === "unassigned") {
              choose(null);
            } else {
              const choice = projects.find((entry) => `project:${entry.id}` === key);
              if (choice) {
                choose(choice.id);
              }
            }
          }}
        >
          <Dropdown.Section
            selectionMode="single"
            selectedKeys={[selectedKey]}
            disallowEmptySelection
          >
            <Header>Project</Header>
            <Dropdown.Item id="unassigned" textValue="No project">
              <Dropdown.ItemIndicator />
              <Label>No project</Label>
            </Dropdown.Item>
            {selected !== undefined && project === undefined ? (
              <Dropdown.Item id={selectedKey} textValue="Unavailable project" isDisabled>
                <Dropdown.ItemIndicator />
                <Label>Unavailable project</Label>
              </Dropdown.Item>
            ) : null}
            {projects.map((entry) => (
              <Dropdown.Item key={entry.id} id={`project:${entry.id}`} textValue={entry.name}>
                <Dropdown.ItemIndicator />
                <Label className="min-w-0 flex-1">
                  <span className="block truncate">{entry.name}</span>
                </Label>
              </Dropdown.Item>
            ))}
          </Dropdown.Section>
          <Separator />
          <Dropdown.Item id="create" textValue="Create project">
            <Plus />
            <Label>Create project</Label>
          </Dropdown.Item>
        </Dropdown.Menu>
      </ActionMenuPopover>
    </Dropdown>
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
      <ChromeButton
        size="navigationHeader"
        appearance="bare"
        className="hidden max-w-40 sm:inline-flex"
        onPress={() => {
          open(id);
        }}
      >
        <span className="truncate">{project?.name ?? "Unavailable project"}</span>
      </ChromeButton>
      <span className="hidden text-muted sm:inline">/</span>
    </>
  );
}
