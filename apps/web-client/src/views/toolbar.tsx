import { useSource } from "@swarmie/client-ui";
import { GripVertical, Maximize2, Minimize2, Plus } from "lucide-react";
import { createPortal } from "react-dom";

import { useClient } from "../client/context";
import { IconButton, iconStyle } from "../controls";
import { startPanelDrag } from "../shell/panel-drag";
import { ViewTabList } from "./view-tabs";

export function ViewToolbar({ toolbarHost }: { toolbarHost: HTMLDivElement | null }) {
  const { shell, openTab, closeTab, expandView } = useClient();
  const layout = useSource(shell);
  const active = layout.tabs.find((tab) => tab.id === layout.tab) ?? layout.tabs[0];
  const movable = layout.conversationVisible && !layout.expanded;
  return (
    <ViewTabList
      tabs={layout.tabs}
      selected={active?.id ?? ""}
      close={closeTab}
      render={(props) => {
        if (toolbarHost === null) {
          return <div {...props} hidden />;
        }
        return createPortal(
          <div
            data-slot="view-toolbar"
            draggable={movable}
            title={movable ? "Drag to move the view panel" : undefined}
            onDragStart={(event) => {
              startPanelDrag(event.dataTransfer, "view");
            }}
            className="flex h-full min-w-0 items-center gap-1 bg-workspace px-1 md:px-3"
          >
            {movable ? (
              <GripVertical className={`${iconStyle} hidden cursor-grab text-muted md:block`} />
            ) : null}
            <div {...props} />
            <IconButton
              icon={Plus}
              label="New view tab"
              onPress={() => {
                openTab();
              }}
            />
            <span className="hidden md:contents">
              <IconButton
                icon={layout.expanded ? Minimize2 : Maximize2}
                label={layout.expanded ? "Restore split view" : "Expand view"}
                onPress={expandView}
              />
            </span>
          </div>,
          toolbarHost,
        );
      }}
    />
  );
}
