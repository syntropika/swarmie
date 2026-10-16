import { ProjectDialog } from "~/src/project-dialog";
import { RenameChatDialog } from "~/src/rename-chat-dialog";
import { WorkspaceSearch } from "~/src/workspace-search";
import type { WorkspaceViewState } from "~/src/workspace-state";

export function WorkspaceDialogs({ view }: { view: WorkspaceViewState }) {
  const {
    store,
    searchScope,
    setSearchScope,
    renaming,
    setRenaming,
    projectDialog,
    setProjectDialog,
    setPanel,
  } = view;
  return (
    <>
      {searchScope !== null && (
        <WorkspaceSearch
          key={searchScope}
          store={store}
          initialScope={searchScope}
          onSelectThread={view.select}
          onNewChat={view.newChat}
          onSettings={(section) => {
            view.setSettingsSection(section);
          }}
          onClose={() => {
            setSearchScope(null);
          }}
          onPanel={setPanel}
          onOpenProject={() => {
            setPanel(null);
            setProjectDialog(true);
          }}
        />
      )}
      {renaming !== null && (
        <RenameChatDialog
          key={renaming.id}
          thread={renaming}
          store={store}
          onClose={() => {
            setRenaming(null);
          }}
        />
      )}
      {projectDialog && (
        <ProjectDialog
          store={store}
          onClose={() => {
            setProjectDialog(false);
          }}
        />
      )}
    </>
  );
}
