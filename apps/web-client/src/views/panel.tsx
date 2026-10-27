import { useSource } from "@swarmie/client-ui";
import type { Tab } from "@swarmie/plugin-views-browser";
import { Tabs } from "@swarmie/ui-web";
import { GripVertical, Maximize2, Minimize2, Plus } from "lucide-react";

import { useClient } from "../client/context";
import { IconButton } from "../controls";
import { ConversationPanel } from "../conversations/conversation";
import { startPanelDrag } from "../shell/panel-drag";
import { StateView } from "./inspector";
import { ViewTabList } from "./view-tabs";

function ViewContent({ tab }: { tab: Tab | undefined }) {
  const { openTab, plugins } = useClient();
  const installed = useSource(plugins);
  if (tab?.kind === "conversation") {
    return <ConversationPanel />;
  }
  if (tab?.kind === "state") {
    return <StateView />;
  }
  if (tab?.kind === "notes") {
    return (
      <div className="mx-auto max-w-[720px] px-7 py-10">
        <h1 className="text-xl font-medium">Project notes</h1>
        <p className="mt-4 text-sm text-muted">
          Project sources will appear here when provided by the connected node.
        </p>
      </div>
    );
  }
  return (
    <div className="mx-auto max-w-xl px-7 pt-14">
      <h2 className="mb-4 text-xs text-muted">Tools</h2>
      <div className="space-y-1">
        {installed.some((plugin) => plugin.id === "app-state" && plugin.enabled) ? (
          <button
            type="button"
            onClick={() => {
              openTab("state");
            }}
            className="flex min-h-8 w-full items-center gap-2 rounded-md px-2 text-left text-xs hover:bg-row-hover"
          >
            App state
          </button>
        ) : null}
      </div>
    </div>
  );
}
export function ViewPanel() {
  const { shell, views, openTab, closeTab, expandView } = useClient();
  const layout = useSource(shell);
  const active = layout.tabs.find((tab) => tab.id === layout.tab) ?? layout.tabs[0];
  return (
    <section aria-label="View" className="flex h-full min-h-0 min-w-0 flex-col bg-pane">
      <Tabs
        appearance="view"
        selectedKey={active?.id ?? ""}
        onSelectionChange={(key) => {
          const tab = layout.tabs.find((entry) => entry.id === key);
          if (tab) {
            views.update((current) => ({ ...current, tab: tab.id }));
          }
        }}
      >
        <div
          data-slot="view-toolbar"
          draggable={layout.conversationVisible && !layout.expanded}
          title={
            layout.conversationVisible && !layout.expanded
              ? "Drag to move the view panel"
              : undefined
          }
          onDragStart={(event) => {
            startPanelDrag(event.dataTransfer, "view");
          }}
          className="flex h-[calc(var(--spacing-chrome-row)+1px)] shrink-0 items-center gap-1 border-b border-border bg-chrome-surface px-1.5"
        >
          {layout.conversationVisible && !layout.expanded ? (
            <GripVertical className="hidden size-3 cursor-grab text-muted md:block" />
          ) : null}
          <ViewTabList tabs={layout.tabs} selected={active?.id ?? ""} close={closeTab} />
          <IconButton
            icon={Plus}
            label="New view tab"
            onPress={() => {
              openTab();
            }}
          />
          <IconButton
            icon={layout.expanded ? Minimize2 : Maximize2}
            label={layout.expanded ? "Restore split view" : "Expand view"}
            onPress={expandView}
          />
        </div>
        <Tabs.Panel key={active?.id ?? ""} id={active?.id ?? ""}>
          <ViewContent tab={active} />
        </Tabs.Panel>
      </Tabs>
    </section>
  );
}
