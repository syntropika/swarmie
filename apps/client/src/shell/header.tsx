import { useSource } from "@swarmie/client-ui";
import {
  ArrowLeft,
  ArrowRight,
  Code2,
  Copy,
  Globe,
  MessageCircle,
  Monitor,
  PanelLeft,
  PanelRight,
  Pin,
  Plus,
} from "lucide-react";

import { useClient } from "../client/context";
import { IconButton, Item, Menu, copyText, iconStyle, useChat } from "../controls";
import { ConversationMenu } from "../conversations/conversation-menu";
import { startPanelDrag } from "./panel-drag";

export function Header() {
  const {
    connection,
    createConversation,
    navigationHistory,
    openTab,
    plugins,
    shell,
    shellState,
    views,
    travel,
  } = useClient();

  const layout = useSource(shell);
  const chat = useChat();
  const status = useSource(connection);
  const enabled = useSource(plugins);
  const history = useSource(navigationHistory);
  return (
    <header className="relative z-40 flex h-11 shrink-0 items-center border-b border-border bg-transparent md:h-8 md:bg-chrome-surface">
      <div
        className={`flex h-full shrink-0 items-center gap-1 px-2 md:border-r md:border-border ${layout.navigation ? "md:w-[calc(var(--navigation-width)+2.625rem)]" : ""}`}
      >
        <span className="hidden items-center gap-1 md:flex">
          <IconButton
            icon={ArrowLeft}
            label="Back"
            disabled={history.index === 0}
            onPress={() => {
              travel(-1);
            }}
          />
          <IconButton
            icon={ArrowRight}
            label="Forward"
            disabled={history.index >= history.entries.length - 1}
            onPress={() => {
              travel(1);
            }}
          />
        </span>
        <NavigationToggle />
        <div className="hidden items-center gap-1 md:flex">
          <Menu label="File menu" text="File" size="action" placement="bottom start">
            {(close) => (
              <>
                <Item
                  icon={Plus}
                  onClick={() => {
                    createConversation();
                    close();
                  }}
                >
                  New conversation
                </Item>
                <Item
                  icon={Globe}
                  onClick={() => {
                    openTab();
                    close();
                  }}
                >
                  New view
                </Item>
              </>
            )}
          </Menu>
          <Menu label="Edit menu" text="Edit" size="action" placement="bottom start">
            {(close) => (
              <Item
                icon={Copy}
                onClick={() => {
                  copyText(
                    chat.messages
                      .getSnapshot()
                      .map((m) => m.text)
                      .join("\n\n"),
                  );
                  close();
                }}
              >
                Copy conversation
              </Item>
            )}
          </Menu>
          <Menu label="View menu" text="View" size="action" placement="bottom start">
            {(close) => (
              <>
                <Item
                  icon={PanelLeft}
                  onClick={() => {
                    shellState.update((s) => ({ ...s, navigation: !s.navigation }));
                    close();
                  }}
                >
                  {layout.navigation ? "Hide navigation" : "Show navigation"}
                </Item>
                <Item
                  icon={Globe}
                  onClick={() => {
                    openTab();
                    close();
                  }}
                >
                  Open view
                </Item>
                <Item
                  icon={PanelRight}
                  onClick={() => {
                    views.update((s) => ({ ...s, conversationVisible: !s.conversationVisible }));
                    close();
                  }}
                >
                  Toggle conversation panel
                </Item>
              </>
            )}
          </Menu>
        </div>
      </div>
      <div
        draggable={layout.section === "conversations" && layout.tabs.length > 0}
        onDragStart={(event) => {
          startPanelDrag(event.dataTransfer, "conversation");
        }}
        className="flex min-w-0 flex-1 items-center gap-2 px-2.5 text-xs md:px-3.5 md:text-chrome"
      >
        <HeaderTitle />
      </div>
      <div className="flex shrink-0 items-center gap-0.5 pr-1.5">
        <span
          title={`Local · ${status}`}
          className="mr-2 hidden items-center gap-1.5 text-panel-switch text-muted md:flex"
        >
          <Monitor className={iconStyle} />
          <span>Local</span>
          <span
            className={`size-1.5 rounded-full ${status === "connected" ? "bg-brand-accent" : "animate-pulse bg-muted"}`}
          />
        </span>
        {layout.section === "conversations" ? (
          <>
            <ConversationMenu />
            <IconButton
              icon={Pin}
              label="Toggle pinned context"
              active={layout.summary}
              onPress={() => {
                shellState.update((s) => ({ ...s, summary: !s.summary }));
              }}
            />
            <IconButton
              icon={layout.tabs.length ? PanelRight : Plus}
              label={layout.tabs.length ? "Toggle conversation panel" : "Open a new view"}
              active={layout.tabs.length > 0 && layout.conversationVisible}
              onPress={() => {
                if (layout.tabs.length > 0) {
                  views.update((s) => ({ ...s, conversationVisible: !s.conversationVisible }));
                } else {
                  openTab();
                }
              }}
            />
          </>
        ) : null}
        {enabled.find((p) => p.id === "app-state")?.enabled === true ? (
          <IconButton
            icon={Code2}
            label="Open App state"
            onPress={() => {
              openTab("state");
            }}
          />
        ) : null}
      </div>
    </header>
  );
}

function HeaderTitle() {
  const { shell, shellUi, conversationsUi } = useClient();
  const layout = useSource(shell);
  const sections = useSource(shellUi.sections);
  const labels = useSource(conversationsUi.header.items);
  const chat = useChat();
  const SectionHeader = sections.find((section) => section.id === layout.section)?.header;
  if (layout.section === "settings") {
    return <span>Settings</span>;
  }
  if (SectionHeader) {
    return <SectionHeader params={layout.params} />;
  }
  return (
    <>
      {labels.length ? (
        labels.map(({ id, component: Label }) => (
          <Label key={id} conversationId={chat.id} isDraft={!chat.started} />
        ))
      ) : (
        <>
          <MessageCircle className={`${iconStyle} text-muted`} />
          <span className="hidden text-muted sm:inline">Chat</span>
          <span className="hidden text-muted sm:inline">/</span>
        </>
      )}
      <span className="truncate font-medium">{chat.title}</span>
    </>
  );
}
function NavigationToggle() {
  const { shell, shellState } = useClient();
  const layout = useSource(shell);
  const control = (
    <IconButton
      icon={PanelLeft}
      label="Toggle navigation"
      active={layout.navigation}
      onPress={() => {
        shellState.update((current) => ({ ...current, navigation: !current.navigation }));
      }}
    />
  );
  return layout.section === "settings" ? <span className="md:hidden">{control}</span> : control;
}
