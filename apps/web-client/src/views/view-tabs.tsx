import type { Tab } from "@swarmie/plugin-views-browser";
import { ChromeButton, Tabs } from "@swarmie/ui-web";
import { Globe, MessageCircle, X } from "lucide-react";
import { createElement, useEffect, useRef } from "react";
import type { ComponentProps } from "react";

import { iconStyle } from "../controls";

export function ViewTabList({
  tabs,
  selected,
  close,
  render,
}: {
  tabs: readonly Tab[];
  selected: string;
  close: (id: string) => void;
  render?: ComponentProps<typeof Tabs.List>["render"];
}) {
  const list = useRef<HTMLDivElement>(null);
  const pendingClose = useRef<string | null>(null);
  const pendingList = useRef<HTMLElement | null>(null);
  useEffect(() => {
    if (pendingClose.current === null || tabs.some((tab) => tab.id === pendingClose.current)) {
      return undefined;
    }
    pendingClose.current = null;
    const element = pendingList.current ?? list.current;
    pendingList.current = null;
    const frame = requestAnimationFrame(() => {
      if (
        element?.isConnected === true &&
        (document.activeElement === document.body || element.contains(document.activeElement))
      ) {
        element.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')?.focus();
      }
    });
    return () => {
      cancelAnimationFrame(frame);
    };
  }, [tabs]);

  useEffect(() => {
    if (!tabs.some((tab) => tab.id === selected)) {
      return undefined;
    }
    const frame = requestAnimationFrame(() => {
      list.current
        ?.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')
        ?.scrollIntoView({ block: "nearest", inline: "nearest" });
    });
    return () => {
      cancelAnimationFrame(frame);
    };
  }, [selected, tabs]);

  function closeTab(id: string, control: Element) {
    const element = control.closest<HTMLElement>('[role="tablist"]') ?? list.current;
    pendingList.current = element;
    if (id !== selected) {
      element?.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')?.focus();
    }
    pendingClose.current = id;
    close(id);
  }

  return (
    <Tabs.List ref={list} aria-label="View tabs" {...(render ? { render } : {})}>
      {tabs.map((tab) => (
        <Tabs.Tab
          key={tab.id}
          id={tab.id}
          aria-label={tab.title}
          render={(props, state) => (
            <div
              data-slot="view-tab"
              data-selected={state.isSelected}
              className="flex items-center"
            >
              {createElement("div", { ...props, title: tab.title })}
              <ChromeButton
                size="tabClose"
                aria-label={`Close ${tab.title}`}
                title={`Close ${tab.title}`}
                tabIndex={state.isSelected ? 0 : -1}
                onPress={(event) => {
                  closeTab(tab.id, event.target);
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
          <Tabs.Indicator />
        </Tabs.Tab>
      ))}
    </Tabs.List>
  );
}
