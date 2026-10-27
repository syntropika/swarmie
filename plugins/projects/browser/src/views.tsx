import { useSource } from "@swarmie/client-ui";
import type { SectionProps } from "@swarmie/plugin-application-shell-browser";
import { Button, ChromeButton, VirtualList } from "@swarmie/ui-web";
import { ArrowLeft, Folder, MessageCircle, Pencil, Plus } from "lucide-react";

import { useProjects } from "./context";

export { ProjectsContext } from "./context";
export type { ProjectsPresentation } from "./context";

export function ProjectsHeader({ params }: SectionProps) {
  const { api, open } = useProjects();
  const state = useSource(api.state);
  const project = state.records.find((entry) => entry.id === params.projectId);
  return (
    <>
      <Folder className="size-3.5 shrink-0 text-muted" />
      <button
        type="button"
        onClick={() => {
          open();
        }}
        className="text-muted hover:text-foreground"
      >
        Projects
      </button>
      {params.projectId === undefined ? null : (
        <>
          <span className="text-muted">/</span>
          <span className="truncate font-medium">{project?.name ?? "Unavailable project"}</span>
        </>
      )}
    </>
  );
}

export function ProjectsPage({ params }: SectionProps) {
  const { api, conversations, open, openConversation, createConversation } = useProjects();
  const state = useSource(api.state);
  const chats = useSource(conversations);
  const project = state.records.find((entry) => entry.id === params.projectId);
  const linked = chats.filter(
    (chat) => chat.started && !chat.archived && state.links[chat.id] === project?.id,
  );
  if (params.projectId !== undefined && project === undefined) {
    return (
      <section className="w-full p-6">
        <h1 className="text-lg font-medium">Project unavailable</h1>
        <p className="mt-2 text-sm text-muted">
          This project is not available in the current account.
        </p>
        <ChromeButton
          size="action"
          onPress={() => {
            open();
          }}
        >
          Back to projects
        </ChromeButton>
      </section>
    );
  }
  return (
    <section
      aria-label={project ? `${project.name} project` : "All projects"}
      className="@container flex min-h-0 w-full flex-col"
    >
      <div className="mx-auto flex w-full max-w-4xl shrink-0 flex-col items-start gap-4 px-5 pt-8 pb-6 @min-[640px]:flex-row @min-[640px]:px-8">
        <div className="w-full min-w-0 flex-1">
          {project ? (
            <button
              type="button"
              className="mb-4 flex items-center gap-1 text-xs text-muted hover:text-foreground"
              onClick={() => {
                open();
              }}
            >
              <ArrowLeft className="size-3" />
              All projects
            </button>
          ) : null}
          <h1 className="text-xl font-medium break-words">{project?.name ?? "Projects"}</h1>
          <p className="mt-2 text-sm leading-6 text-muted">
            {project
              ? project.description || "Conversations in this project."
              : "Organize your conversations by project."}
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-1 self-end pt-0.5 @min-[640px]:self-start">
          {project ? (
            <>
              <ChromeButton
                size="action"
                aria-label="Edit project"
                onPress={() => {
                  api.showEdit(project.id);
                }}
              >
                <Pencil className="size-3.5" />
              </ChromeButton>
              <Button
                size="sm"
                tone="brand"
                shape="panel"
                onPress={() => {
                  createConversation(project.id).catch(api.fail);
                }}
              >
                <Plus className="size-3.5" />
                New conversation
              </Button>
            </>
          ) : (
            <Button size="sm" tone="brand" shape="panel" onPress={api.showCreate}>
              <Plus className="size-3.5" />
              New project
            </Button>
          )}
        </div>
      </div>
      {state.error === null ? null : (
        <p role="alert" className="mx-auto w-full max-w-4xl px-5 pb-3 text-xs text-danger">
          {state.error}
        </p>
      )}
      {project ? (
        <div className="mx-auto flex min-h-0 w-full max-w-4xl flex-1 flex-col px-5 pb-5 @min-[640px]:px-8">
          <h2 className="shrink-0 border-b border-border pb-3 text-xs font-medium">
            Conversations <span className="ml-1 text-muted">{linked.length}</span>
          </h2>
          {linked.length ? (
            <VirtualList
              items={linked}
              getItemKey={(chat) => chat.id}
              label="Project conversations"
              estimateSize={56}
              className="min-h-0 flex-1"
              renderItem={(chat) => (
                <button
                  type="button"
                  onClick={() => {
                    openConversation(chat.id);
                  }}
                  className="flex min-h-14 w-full items-center gap-3 border-b border-border px-2 text-left text-sm hover:bg-row-hover"
                >
                  <MessageCircle className="size-4 shrink-0 text-muted" />
                  <span className="truncate">{chat.title}</span>
                </button>
              )}
            />
          ) : (
            <p className="py-8 text-sm text-muted">No conversations in this project yet.</p>
          )}
        </div>
      ) : (
        <div className="mx-auto min-h-0 w-full max-w-4xl flex-1 overflow-auto px-5 pb-6 @min-[640px]:px-8">
          {state.records
            .filter((entry) => !entry.archived)
            .map((entry) => (
              <button
                key={entry.id}
                type="button"
                onClick={() => {
                  open(entry.id);
                }}
                className="flex w-full items-center gap-3 border-b border-border px-2 py-4 text-left hover:bg-row-hover"
              >
                <Folder className="size-4 shrink-0 text-muted" />
                <div className="min-w-0 flex-1">
                  <p className="truncate text-sm font-medium">{entry.name}</p>
                  {entry.description ? (
                    <p className="mt-1 truncate text-xs text-muted">{entry.description}</p>
                  ) : null}
                </div>
                <span className="text-xs text-muted">
                  {
                    chats.filter(
                      (chat) => chat.started && !chat.archived && state.links[chat.id] === entry.id,
                    ).length
                  }{" "}
                  chats
                </span>
              </button>
            ))}
          {state.records.length === 0 ? (
            <div className="py-8 text-sm text-muted">
              No projects yet. Create a project to organize your conversations.
            </div>
          ) : null}
        </div>
      )}
    </section>
  );
}

export { ProjectsSidebar } from "./sidebar";
export { ProjectDialog } from "./editor";
