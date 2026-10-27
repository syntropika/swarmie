import { useSource } from "@swarmie/client-ui";
import type { Tab } from "@swarmie/plugin-views-browser";
import { Globe, GripVertical, Maximize2, MessageCircle, Minimize2, Plus, X } from "lucide-react";

import { useClient } from "../client/context";
import { IconButton } from "../controls";
import { ConversationPanel } from "../conversations/conversation";
import { startPanelDrag } from "../shell/panel-drag";
import { StateView } from "./inspector";

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
      <div
        draggable={layout.conversationVisible && !layout.expanded}
        title={
          layout.conversationVisible && !layout.expanded ? "Drag to move the view panel" : undefined
        }
        onDragStart={(event) => {
          startPanelDrag(event.dataTransfer, "view");
        }}
        className="flex h-9 shrink-0 items-center gap-1 border-b border-border bg-chrome-surface px-1.5"
      >
        {layout.conversationVisible && !layout.expanded ? (
          <GripVertical className="hidden size-3 cursor-grab text-muted md:block" />
        ) : null}
        <div
          role="tablist"
          aria-label="View tabs"
          className="flex h-full min-w-0 flex-1 [scrollbar-width:none] items-end gap-1 overflow-x-auto overflow-y-hidden"
        >
          {layout.tabs.map((tab) => (
            <div
              key={tab.id}
              className={`flex h-7 max-w-48 shrink-0 items-center rounded-t-md ${tab.id === active?.id ? "bg-row-selected" : "hover:bg-row-hover"}`}
            >
              <button
                type="button"
                role="tab"
                aria-selected={tab.id === active?.id}
                onClick={() => {
                  views.update((current) => ({ ...current, tab: tab.id }));
                }}
                className="flex min-w-0 items-center gap-2 px-2 text-chrome"
              >
                {tab.kind === "conversation" ? (
                  <MessageCircle className="size-3 text-muted" />
                ) : (
                  <Globe className="size-3 text-muted" />
                )}
                <span className="truncate">{tab.title}</span>
              </button>
              <IconButton
                icon={X}
                size="tabClose"
                label={`Close ${tab.title}`}
                onPress={() => {
                  closeTab(tab.id);
                }}
              />
            </div>
          ))}
        </div>
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
      <div role="tabpanel" aria-label={active?.title ?? "New tab"} className="min-h-0 flex-1">
        <ViewContent tab={active} />
      </div>
    </section>
  );
}
