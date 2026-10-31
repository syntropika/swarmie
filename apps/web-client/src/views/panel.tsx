import { useSource } from "@swarmie/client-ui";
import type { Tab } from "@swarmie/plugin-browser/views";
import { Button, Tabs } from "@swarmie/ui-web";

import { useClient } from "../client/context";
import { ConversationPanel } from "../conversations/conversation";
import { GitCapabilityView, GitLauncher } from "../git/workspace";
import { StateView } from "./inspector";
import { ViewToolbar } from "./toolbar";

function ViewContent({ tab }: { tab: Tab | undefined }) {
  const { openTab, plugins, roster, gitPresentation } = useClient();
  const installed = useSource(plugins);
  const git = useSource(gitPresentation.state);
  const gitAvailable = git.enabled && Object.values(git.nodes).some((node) => node.available);
  const conversations = useSource(roster);
  if (tab?.kind === "capability") {
    return tab.owner === "git" ? (
      <GitCapabilityView tab={tab} />
    ) : (
      <output className="block p-6 text-sm text-muted">
        The capability for this view is unavailable.
      </output>
    );
  }
  if (tab?.kind === "conversation") {
    const chat = conversations.find((entry) => entry.id === tab.conversationId);
    return chat ? (
      <ConversationPanel chat={chat} />
    ) : (
      <output className="block p-4 text-ui text-muted">Conversation unavailable.</output>
    );
  }
  if (tab?.kind === "state") {
    return <StateView />;
  }
  if (tab?.kind === "notes") {
    return (
      <div className="mx-auto max-w-[720px] px-7 py-10">
        <h1 className="text-xl font-medium">Project notes</h1>
        <p className="mt-4 text-ui text-muted">
          Project sources will appear here when provided by the connected node.
        </p>
      </div>
    );
  }
  return (
    <div className="h-full overflow-auto">
      {gitAvailable ? <GitLauncher /> : null}
      <div className="mx-auto max-w-xl px-7 pb-7">
        <h2 className="mb-4 text-navigation-heading text-muted">Tools</h2>
        <div className="space-y-1">
          {installed.some((plugin) => plugin.id === "app-state" && plugin.enabled) ? (
            <Button
              variant="ghost"
              onPress={() => {
                openTab("state");
              }}
              className="w-full justify-start"
            >
              App state
            </Button>
          ) : null}
        </div>
      </div>
    </div>
  );
}
export function ViewPanel({ toolbarHost }: { toolbarHost: HTMLDivElement | null }) {
  const { shell, views } = useClient();
  const layout = useSource(shell);
  const active = layout.tabs.find((tab) => tab.id === layout.tab) ?? layout.tabs[0];
  return (
    <section
      aria-label="View"
      data-slot="view-panel"
      className="swarmie-view-tabs flex h-full min-h-0 min-w-0 flex-col overflow-hidden bg-pane shadow-sheet"
    >
      <Tabs
        className="h-full min-h-0 min-w-0"
        selectedKey={active?.id ?? ""}
        onSelectionChange={(key) => {
          const tab = layout.tabs.find((entry) => entry.id === key);
          if (tab) {
            views.update((current) => ({ ...current, tab: tab.id }));
          }
        }}
      >
        <ViewToolbar toolbarHost={toolbarHost} />
        <Tabs.Panel key={active?.id ?? ""} id={active?.id ?? ""} className="m-0 min-h-0 flex-1">
          <ViewContent tab={active} />
        </Tabs.Panel>
      </Tabs>
    </section>
  );
}
