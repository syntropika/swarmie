import { useSource } from "@swarmie/client-ui";
import type { Conversation } from "@swarmie/plugin-conversations-browser";
import { BrandMark, ChromeButton } from "@swarmie/ui-web";
import {
  Archive,
  Bell,
  ChevronDown,
  HelpCircle,
  LogIn,
  MessageCircle,
  Monitor,
  Pin,
  Plus,
  Puzzle,
  Settings,
} from "lucide-react";
import type { ReactNode } from "react";
import { useState } from "react";

import { useClient } from "../client/context";
import { IconButton, Item, Menu, iconStyle } from "../controls";
import { ResizeHandle } from "./resize";

export function Navigation() {
  const {
    archive,
    createConversation,
    navigate,
    notifications,
    openConversation,
    openTab,
    plugins,
    roster,
    shell,
    togglePin,
    conversationsUi,
    shellUi,
    shellState,
  } = useClient();

  const layout = useSource(shell);
  const chats = useSource(roster).filter((c) => c.started && !c.archived);
  const enabled = useSource(plugins);
  const updates = useSource(notifications);
  const [mode, setMode] = useState<"chats" | "notifications">("chats");
  const [folded, setFolded] = useState<string[]>([]);
  const groups = useSource(conversationsUi.navigation.groups);
  const sections = useSource(shellUi.sections);
  const grouped = new Set(groups.flatMap((entry) => entry.membership.getSnapshot()));
  const hidden = !layout.navigation;
  const group = (id: string, title: string, content: ReactNode, extras?: ReactNode) => (
    <section className="mb-5">
      <div className="mb-1 flex h-7 items-center gap-1 px-1">
        <button
          type="button"
          aria-expanded={!folded.includes(id)}
          onClick={() => {
            setFolded((old) => (old.includes(id) ? old.filter((v) => v !== id) : [...old, id]));
          }}
          className="flex flex-1 items-center gap-1 text-left text-chrome text-muted hover:text-foreground"
        >
          {title}
          <ChevronDown className={`size-3 ${folded.includes(id) ? "-rotate-90" : ""}`} />
        </button>
        {extras}
      </div>
      {folded.includes(id) ? null : content}
    </section>
  );
  const rows = (items: Conversation[]) =>
    items.map((chat) => (
      <div
        key={chat.id}
        className={`group flex min-h-8 items-center rounded-md ${layout.active === chat.id ? "bg-row-selected" : "hover:bg-row-hover"}`}
      >
        <button
          type="button"
          onClick={() => {
            openConversation(chat.id);
            if (window.innerWidth < 768) {
              shellState.update((s) => ({ ...s, navigation: false }));
            }
          }}
          className="min-w-0 flex-1 truncate px-2 py-2 text-left text-xs"
        >
          {chat.title}
        </button>
        <div className="mr-1 hidden items-center group-focus-within:flex group-hover:flex">
          <IconButton
            icon={Pin}
            size="tabClose"
            label={`Pin ${chat.title}`}
            active={chat.pinned}
            onPress={() => {
              togglePin(chat);
            }}
          />
          <IconButton
            icon={Archive}
            size="tabClose"
            label={`Archive ${chat.title}`}
            onPress={() => {
              archive(chat);
            }}
          />
        </div>
      </div>
    ));
  return (
    <>
      {layout.navigation ? (
        <button
          aria-label="Close navigation"
          type="button"
          onClick={() => {
            shellState.update((s) => ({ ...s, navigation: false }));
          }}
          className="fixed inset-0 z-20 bg-background/60 md:hidden"
        />
      ) : null}
      <div
        className={`z-30 flex shrink-0 border-r border-border bg-navigation-surface ${hidden ? "hidden md:flex" : "fixed inset-y-11 left-0 md:static"}`}
      >
        <nav
          aria-label="Application"
          className={`flex w-10.5 shrink-0 flex-col items-center border-r border-border py-2 ${hidden ? "hidden md:flex" : ""}`}
        >
          <button
            type="button"
            aria-label="Swarmie conversations"
            onClick={() => {
              navigate({ section: "conversations" });
            }}
            className="mb-6 size-7"
          >
            <BrandMark className="size-7 [&_img]:size-7" />
          </button>
          <IconButton
            size="rail"
            icon={MessageCircle}
            label="Conversations"
            active={layout.section === "conversations"}
            onPress={() => {
              navigate({ section: "conversations" });
            }}
          />
          {sections.map((section) => (
            <IconButton
              key={section.id}
              size="rail"
              icon={section.icon}
              label={section.label}
              active={layout.section === section.id}
              onPress={() => {
                navigate({ section: section.id, params: {} });
                if (window.innerWidth < 768) {
                  shellState.update((current) => ({ ...current, navigation: false }));
                }
              }}
            />
          ))}
          <div className="mt-auto">
            <Menu label="Account menu" icon={Settings} size="application" placement="top start">
              {(close) => (
                <>
                  <Item
                    icon={Settings}
                    onClick={() => {
                      navigate({ section: "settings" });
                      shellState.update((s) => ({ ...s, navigation: window.innerWidth >= 768 }));
                      close();
                    }}
                  >
                    Settings
                  </Item>
                  <Item
                    icon={HelpCircle}
                    onClick={() => {
                      openTab("notes");
                      close();
                    }}
                  >
                    Help
                  </Item>
                  <div className="my-1 border-t border-border" />
                  <Item
                    icon={LogIn}
                    onClick={() => {
                      navigate({ section: "settings", settings: "general" });
                      close();
                    }}
                  >
                    Sign in to Swarmie
                  </Item>
                </>
              )}
            </Menu>
          </div>
        </nav>
        {hidden ? null : (
          <aside className="flex w-(--navigation-width) flex-col overflow-hidden">
            {layout.section === "settings" ? (
              <>
                <div className="flex h-9 shrink-0 items-center border-b border-border px-3 text-xs font-medium">
                  Settings
                </div>
                <div className="space-y-1 p-2 pt-4">
                  {(
                    [
                      ["general", Settings, "General"],
                      ["connections", Monitor, "Connections"],
                      ["providers", Puzzle, "Providers"],
                      ["plugins", Puzzle, "Plugins"],
                    ] as const
                  ).map(([id, Icon, title]) => (
                    <ChromeButton
                      key={id}
                      size="navigation"
                      selected={layout.settings === id}
                      onPress={() => {
                        navigate({ settings: id });
                        if (window.innerWidth < 768) {
                          shellState.update((s) => ({ ...s, navigation: false }));
                        }
                      }}
                    >
                      <Icon className={iconStyle} />
                      {title}
                    </ChromeButton>
                  ))}
                </div>
              </>
            ) : (
              <>
                <div className="flex h-9 shrink-0 items-center gap-0.5 border-b border-border px-1.5">
                  <ChromeButton
                    size="action"
                    className="min-w-0 flex-1"
                    onPress={() => {
                      createConversation();
                      if (window.innerWidth < 768) {
                        shellState.update((s) => ({ ...s, navigation: false }));
                      }
                    }}
                  >
                    <Plus className={iconStyle} />
                    <span className="truncate">New conversation</span>
                  </ChromeButton>
                  {enabled.find((p) => p.id === "notifications")?.enabled === true ? (
                    <IconButton
                      icon={Bell}
                      label="Toggle notifications"
                      active={mode === "notifications"}
                      onPress={() => {
                        setMode((old) => (old === "chats" ? "notifications" : "chats"));
                      }}
                    />
                  ) : null}
                </div>
                <div className="min-h-0 flex-1 overflow-auto px-2 py-4">
                  {mode === "notifications" &&
                  enabled.find((p) => p.id === "notifications")?.enabled === true ? (
                    <>
                      <div className="mb-3 px-1 text-chrome text-muted">Notifications</div>
                      {updates.length ? (
                        updates.map((update) => (
                          <button
                            key={update.id}
                            type="button"
                            onClick={() => {
                              if (update.conversationId !== undefined) {
                                openConversation(update.conversationId);
                              }
                              setMode("chats");
                            }}
                            className="mb-1 w-full rounded-md p-2 text-left hover:bg-row-hover"
                          >
                            <div className="truncate text-xs">{update.title}</div>
                            <div className="mt-1 text-chrome text-muted">{update.text}</div>
                          </button>
                        ))
                      ) : (
                        <p className="px-1 text-xs leading-6 text-muted">
                          Conversation updates will appear here.
                        </p>
                      )}
                    </>
                  ) : (
                    <>
                      {chats.some((c) => c.pinned)
                        ? group("pinned", "Pinned", rows(chats.filter((c) => c.pinned)))
                        : null}
                      {groups.map(({ id, component: Group }) => (
                        <Group
                          key={id}
                          conversations={chats}
                          activeId={layout.active}
                          openConversation={openConversation}
                          renderConversations={(items) => {
                            const ids = new Set(items.map((item) => item.id));
                            return rows(chats.filter((chat) => ids.has(chat.id)));
                          }}
                        />
                      ))}
                      {group(
                        "recent",
                        "Recent chats",
                        rows(chats.filter((c) => !grouped.has(c.id) && !c.pinned)),
                      )}
                    </>
                  )}
                </div>
              </>
            )}
          </aside>
        )}
      </div>
      {layout.navigation ? <ResizeHandle target="navigation" /> : null}
    </>
  );
}
