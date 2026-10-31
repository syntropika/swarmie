import { useSource } from "@swarmie/client-ui";
import type { SectionProps } from "@swarmie/plugin-browser/shell";
import { Breadcrumbs, Button, Description, Label, ListBox, VirtualList } from "@swarmie/ui-web";
import { ArrowLeft, Folder, MessageCircle, Pencil, Plus } from "lucide-react";
import { useMemo, useState } from "react";

import { useProjects } from "./context";
import { savedBrowserProjects } from "./persistence";

export { ProjectsContext } from "./context";
export type { ProjectsPresentation } from "./context";

export function ProjectsHeader({ params }: SectionProps) {
  const { api, open } = useProjects();
  const state = useSource(api.state);
  const project = state.records.find((entry) => entry.id === params.projectId);
  return (
    <>
      <Folder className="size-3.5 shrink-0 text-muted" />
      <Breadcrumbs
        aria-label="Project path"
        separator={<span aria-hidden="true">/</span>}
        className="min-w-0"
      >
        <Breadcrumbs.Item
          onPress={() => {
            open();
          }}
          aria-current={params.projectId === undefined ? "page" : false}
        >
          Projects
        </Breadcrumbs.Item>
        {params.projectId === undefined ? null : (
          <Breadcrumbs.Item aria-current="page">
            <span className="min-w-0 truncate">{project?.name ?? "Unavailable project"}</span>
          </Breadcrumbs.Item>
        )}
      </Breadcrumbs>
    </>
  );
}

export function ProjectsPage({ params }: SectionProps) {
  const { api, conversations, open, openConversation, createConversation, browserProfileKey } =
    useProjects();
  const state = useSource(api.state);
  const chats = useSource(conversations);
  const project = state.records.find((entry) => entry.id === params.projectId);
  const linked = chats.filter(
    (chat) => chat.started && !chat.archived && state.links[chat.id] === project?.id,
  );
  if (params.projectId !== undefined && project === undefined) {
    return (
      <section className="w-full min-w-0 px-(--spacing-panel-inset) py-6">
        <h1 className="text-lg font-medium">Project unavailable</h1>
        <p className="mt-2 text-sm text-muted">
          This project is not available in the current account.
        </p>
        <Button
          variant="ghost"
          onPress={() => {
            open();
          }}
        >
          Back to projects
        </Button>
      </section>
    );
  }
  return (
    <section
      aria-label={project ? `${project.name} project` : "All projects"}
      className="@container flex min-h-0 w-full min-w-0 flex-col"
    >
      <div className="mx-auto flex w-full max-w-4xl shrink-0 flex-col items-start gap-4 px-(--spacing-panel-inset) pt-6 pb-5 @min-[640px]:flex-row @min-[640px]:px-8 @min-[640px]:pt-8 @min-[640px]:pb-6">
        <div className="w-full min-w-0 flex-1">
          {project ? (
            <Button
              variant="ghost"
              className="mb-4"
              onPress={() => {
                open();
              }}
            >
              <ArrowLeft />
              All projects
            </Button>
          ) : null}
          <h1 className="text-2xl font-medium break-words">{project?.name ?? "Projects"}</h1>
          <p className="mt-2 text-sm leading-6 break-words text-muted">
            {project
              ? project.description || "Conversations in this project."
              : "Organize your conversations by project."}
          </p>
        </div>
        <div className="flex max-w-full shrink-0 flex-wrap items-center gap-1 @min-[640px]:pt-0.5">
          {project ? (
            <>
              <Button
                variant="ghost"
                isIconOnly
                aria-label="Edit project"
                onPress={() => {
                  api.showEdit(project.id);
                }}
              >
                <Pencil />
              </Button>
              <Button
                size="sm"

                onPress={() => {
                  createConversation(project.id).catch(api.fail);
                }}
              >
                <Plus />
                New conversation
              </Button>
            </>
          ) : (
            <Button size="sm" onPress={api.showCreate}>
              <Plus />
              New project
            </Button>
          )}
        </div>
      </div>
      {project === undefined && browserProfileKey !== undefined ? (
        <BrowserProjectImport profileKey={browserProfileKey} />
      ) : null}
      {state.error === null ? null : (
        <p
          role="alert"
          className="mx-auto w-full max-w-4xl px-(--spacing-panel-inset) pb-3 text-xs break-words text-danger @min-[640px]:px-8"
        >
          {state.error}
        </p>
      )}
      {project ? (
        <div className="mx-auto flex min-h-0 w-full max-w-4xl flex-1 flex-col px-(--spacing-panel-inset) pb-5 @min-[640px]:px-8">
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
                <Button
                  variant="ghost"
                  onPress={() => {
                    openConversation(chat.id);
                  }}
                  className="w-full justify-start"
                >
                  <MessageCircle className="size-4 shrink-0 text-muted" />
                  <span className="truncate">{chat.title}</span>
                </Button>
              )}
            />
          ) : (
            <p className="py-4 text-sm text-muted @min-[640px]:py-8">
              No conversations in this project yet.
            </p>
          )}
        </div>
      ) : (
        <div className="mx-auto min-h-0 w-full max-w-4xl flex-1 overflow-auto px-(--spacing-panel-inset) pb-6 @min-[640px]:px-8">
          <ListBox
            aria-label="Projects"
            selectionMode="none"
            onAction={(key) => {
              if (typeof key === "string") {
                open(key);
              }
            }}
          >
            {state.records
              .filter((entry) => !entry.archived)
              .map((entry) => (
                <ListBox.Item key={entry.id} id={entry.id} textValue={entry.name}>
                  <Folder />
                  <div className="grid min-w-0 flex-1 gap-1">
                    <Label>{entry.name}</Label>
                    {entry.description ? <Description>{entry.description}</Description> : null}
                  </div>
                  <span className="shrink-0 text-xs whitespace-nowrap text-muted">
                    {
                      chats.filter(
                        (chat) =>
                          chat.started && !chat.archived && state.links[chat.id] === entry.id,
                      ).length
                    }{" "}
                    chats
                  </span>
                </ListBox.Item>
              ))}
          </ListBox>
          {state.records.length === 0 ? (
            <div className="py-4 text-sm text-muted @min-[640px]:py-8">
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

function BrowserProjectImport({ profileKey }: { profileKey: string }) {
  const { api } = useProjects();
  const state = useSource(api.state);
  const source = useMemo(() => {
    try {
      return { data: savedBrowserProjects(profileKey), error: null };
    } catch (error: unknown) {
      return { data: null, error: error instanceof Error ? error.message : String(error) };
    }
  }, [profileKey]);
  const [busy, setBusy] = useState(false);
  async function importProjects() {
    if (source.data === null) {
      return;
    }
    setBusy(true);
    try {
      await api.importData(source.data);
    } catch (error: unknown) {
      api.fail(error);
    } finally {
      setBusy(false);
    }
  }
  function requestImport() {
    importProjects().catch(api.fail);
  }
  if (source.error !== null) {
    return (
      <p role="alert" className="px-4 text-sm text-danger">
        {source.error}
      </p>
    );
  }
  if (source.data === null || source.data.records.length === 0 || state.records.length !== 0) {
    return null;
  }
  return (
    <div className="mx-auto w-full max-w-4xl px-4 pb-4">
      <p className="mb-2 text-sm text-muted">
        Projects saved in this browser can be imported into the connected runtime. The browser copy
        will be kept.
      </p>
      <Button variant="secondary" isDisabled={!state.available || busy} onPress={requestImport}>
        Import browser projects
      </Button>
    </div>
  );
}
