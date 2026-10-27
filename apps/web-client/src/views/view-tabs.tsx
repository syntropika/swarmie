import type { Tab } from "@swarmie/plugin-views-browser";
import { ChromeButton, Tabs } from "@swarmie/ui-web";
import { Globe, MessageCircle, X } from "lucide-react";
import { createElement, useEffect, useRef } from "react";

import { iconStyle } from "../controls";

export function ViewTabList({
  tabs,
  selected,
  close,
}: {
  tabs: readonly Tab[];
  selected: string;
  close: (id: string) => void;
}) {
  const list = useRef<HTMLDivElement>(null);
  const pendingClose = useRef<string | null>(null);
  useEffect(() => {
    if (pendingClose.current === null || tabs.some((tab) => tab.id === pendingClose.current)) {
      return;
    }
    pendingClose.current = null;
    const element = list.current;
    if (
      element &&
      (document.activeElement === document.body || element.contains(document.activeElement))
    ) {
      element.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')?.focus();
    }
  }, [tabs]);

  function closeTab(id: string) {
    if (id !== selected) {
      list.current?.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')?.focus();
    }
    pendingClose.current = id;
    close(id);
  }

  return (
    <Tabs.List ref={list} aria-label="View tabs">
      {tabs.map((tab) => (
        <Tabs.Tab
          key={tab.id}
          id={tab.id}
          aria-label={tab.title}
          render={(props, state) => (
            <div
              className={`flex h-(--spacing-chrome-row) max-w-48 shrink-0 items-center rounded-t-md ${state.isSelected ? "bg-row-selected" : "hover:bg-row-hover"}`}
            >
              {createElement("div", { ...props, title: tab.title })}
              <ChromeButton
                size="tabClose"
                aria-label={`Close ${tab.title}`}
                title={`Close ${tab.title}`}
                tabIndex={state.isSelected ? 0 : -1}
                onPress={() => {
                  closeTab(tab.id);
                }}
              >
                <X className={iconStyle} />
              </ChromeButton>
            </div>
          )}
        >
          {tab.kind === "conversation" ? (
            <MessageCircle className={iconStyle} />
          ) : (
            <Globe className={iconStyle} />
          )}
          <span className="truncate">{tab.title}</span>
        </Tabs.Tab>
      ))}
    </Tabs.List>
  );
}
