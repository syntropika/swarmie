import { SearchDialog } from "@swarmie/ui-web";
import type { SearchDialogSection } from "@swarmie/ui-web";
import { useState, useSyncExternalStore } from "react";

import { useAccount } from "~/src/account-state";
import { startAction, errorMessage } from "~/src/action-boundary";
import { useContributions } from "~/src/contribution-host";
import { visibleSettingsSections } from "~/src/settings-sections";
import type { SettingsSection } from "~/src/settings-sections";
import type { WorkspaceStore } from "~/src/workspace";
import { useWorkspaceSearchFiles } from "~/src/workspace-search-files";
import {
  chatSearchSections,
  fileSearchItems,
  matchesSearch,
  scopeSearchSection,
  searchContexts,
  searchNavigation,
} from "~/src/workspace-search-items";
import type { SearchAction, SearchScope } from "~/src/workspace-search-items";
import type { Panel } from "~/src/workspace-state";

export function WorkspaceSearch({
  store,
  initialScope = "all",
  onClose,
  onPanel,
  onOpenProject,
  onSelectThread,
  onNewChat,
  onSettings,
}: {
  store: WorkspaceStore;
  initialScope?: SearchScope;
  onClose: () => void;
  onPanel: (panel: Exclude<Panel, null>) => void;
  onOpenProject: () => void;
  onSelectThread?: (id: string) => void;
  onNewChat?: (projectId?: string) => void;
  onSettings?: (section: SettingsSection) => void;
}) {
  const { account } = useAccount();
  const settings: SearchAction[] =
    onSettings === undefined
      ? []
      : visibleSettingsSections(account?.status === "signedIn").map((section) => ({
          id: `settings:${section.id}`,
          label: section.title,
          icon: <section.icon size={16} />,
          run: () => {
            onSettings(section.id);
            onPanel("settings");
          },
        }));
  const state = useSyncExternalStore(store.subscribe, store.getSnapshot);
  const host = useContributions();
  useSyncExternalStore(
    host?.registry.subscribe ?? subscribeWithoutHost,
    host?.registry.getSnapshot ?? emptySnapshot,
  );
  const [scope, setScope] = useState(initialScope);
  const [query, setQuery] = useState("");
  const filesEnabled = scope === "files" || (scope === "all" && query.trim().length > 0);
  const files = useWorkspaceSearchFiles(store, query, filesEnabled);
  const newChat = (projectId?: string) => {
    if (onNewChat === undefined) {
      store.createThread(projectId);
    } else {
      onNewChat(projectId);
    }
  };
  const openScope = (next: SearchScope) => {
    setQuery("");
    setScope(next);
  };
  const navigation = searchNavigation(openScope);
  const commands: SearchAction[] = [
    {
      id: "action:new",
      label: "New chat",
      run: () => {
        newChat();
      },
    },
    { id: "action:projects", label: "Open project", run: onOpenProject },
    {
      id: "action:tools",
      label: "Workspace tools",
      run: () => {
        onPanel("extensions");
      },
    },
    {
      id: "action:review",
      label: "Review changes",
      run: () => {
        onPanel("review");
        startAction(store.review(), store.setError.bind(store));
      },
    },
    {
      id: "action:connection",
      label: "Settings",
      run: () => {
        onSettings?.("general");
        onPanel("settings");
      },
    },
    ...(host?.registry.commands() ?? []).map((command) => ({
      id: `extension:${command.key}`,
      label: command.title,
      run: () => {
        host?.registry.run(command.key);
      },
    })),
  ];
  const projects: SearchAction[] = state.projects.map((project) => ({
    id: `project:${project.id}`,
    label: project.name,
    description: project.path,
    run: () => {
      newChat(project.id);
    },
  }));
  const sections = searchSections(
    scope,
    query,
    navigation,
    commands,
    projects,
    chatSearchSections(state.threads, query),
    fileSearchItems(files.paths),
    settings,
  );
  const onAction = (id: string) => {
    const navigate = navigation.find((action) => action.id === id);
    if (navigate !== undefined) {
      navigate.run();
      return;
    }
    try {
      if (id.startsWith("chat:")) {
        if (onSelectThread === undefined) {
          store.select(id.slice(5));
        } else {
          onSelectThread(id.slice(5));
        }
      } else if (id.startsWith("file:")) {
        store.openFile({ path: id.slice(5) });
      } else {
        [...commands, ...projects, ...settings].find((action) => action.id === id)?.run();
      }
      onClose();
    } catch (error) {
      store.setError(error);
    }
  };
  const context = searchContexts[scope];
  return (
    <SearchDialog
      title="Search workspace"
      inputLabel={context.label}
      placeholder={context.placeholder}
      query={query}
      onQueryChange={setQuery}
      sections={sections}
      onAction={onAction}
      onClose={onClose}
      context={
        scope === "all"
          ? undefined
          : {
              label: context.label,
              onBack: () => {
                openScope("all");
              },
            }
      }
      emptyMessage={searchEmptyMessage(scope, query, files.available, files.loading)}
      emptyAction={
        scope === "chats"
          ? {
              label: "New chat",
              onPress: () => {
                onAction("action:new");
              },
            }
          : undefined
      }
      status={
        files.error === null && !files.loading ? undefined : (
          <FileSearchStatus error={files.error} loading={files.loading} />
        )
      }
    />
  );
}

function searchSections(
  scope: SearchScope,
  query: string,
  navigation: SearchAction[],
  commands: SearchAction[],
  projects: SearchAction[],
  chats: SearchDialogSection[],
  files: ReturnType<typeof fileSearchItems>,
  settings: SearchAction[],
): SearchDialogSection[] {
  const groups: SearchDialogSection[] = [];
  if (scope === "all") {
    groups.push(scopeSearchSection(navigation, query));
  }
  if (scope === "all" || scope === "chats") {
    groups.push(...chats);
  }
  if (scope === "all") {
    groups.push({
      id: "projects",
      label: "Projects",
      items: projects.filter(
        (project) =>
          matchesSearch(project.label, query) || matchesSearch(project.description ?? "", query),
      ),
    });
  }
  if (scope === "all" || scope === "commands") {
    groups.push({
      id: "commands",
      label: "Commands",
      items: commands.filter((command) => matchesSearch(command.label, query)),
    });
  }
  if (scope === "all" || scope === "commands") {
    groups.push({
      id: "settings",
      label: "Settings",
      items: settings.filter((action) => matchesSearch(action.label, query)),
    });
  }
  if (scope === "all" || scope === "files") {
    groups.push({ id: "files", label: "Project files", items: files });
  }
  return groups;
}

function searchEmptyMessage(
  scope: SearchScope,
  query: string,
  filesAvailable: boolean,
  loading: boolean,
) {
  if (scope === "files" && !filesAvailable) {
    return "Open a project to search its files.";
  }
  if (loading) {
    return "Searching project files…";
  }
  if (scope === "chats" && query.trim().length === 0) {
    return "No chats to search yet.";
  }
  return `No results for “${query}”.`;
}

function subscribeWithoutHost() {
  return unsubscribeWithoutHost;
}
function unsubscribeWithoutHost() {
  /* There is no registry subscription to release. */
}
function emptySnapshot() {
  return 0;
}

function FileSearchStatus({ error, loading }: { error: Error | null; loading: boolean }) {
  if (error !== null) {
    return (
      <p role="alert" className="text-danger">
        {errorMessage(error)}
      </p>
    );
  }
  return loading ? <output>Searching project files…</output> : null;
}
