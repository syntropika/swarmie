import { useSource } from "@swarmie/client-ui";
import { Breadcrumbs, ChromeSeparator } from "@swarmie/ui-web";
import {
  ArrowLeft,
  ArrowRight,
  Copy,
  Globe,
  Logs,
  MessageCircle,
  PanelLeft,
  PanelRight,
  Plus,
} from "lucide-react";

import { useClient } from "../client/context";
import { IconButton, Item, Menu, copyText, useChat } from "../controls";
import { ConversationMenu } from "../conversations/conversation-menu";
import { startPanelDrag } from "./panel-drag";

const headerIconStyle = "size-(--spacing-chrome-icon) shrink-0";

export function Header({
  onViewToolbarHost,
}: {
  onViewToolbarHost: (node: HTMLDivElement | null) => void;
}) {
  const { shell } = useClient();
  const layout = useSource(shell);
  const views = layout.section === "conversations" && layout.tabs.length > 0;
  const conversation = !views || layout.conversationVisible;
  const split = views && conversation && !layout.expanded;
  return (
    <header className="swarmie-header relative z-40 flex h-(--spacing-shell-bar) shrink-0 items-center bg-workspace">
      <HeaderNavigation />
      <div data-slot="workspace-header" className="relative flex h-full min-w-0 flex-1">
        <ChromeSeparator className="absolute top-1/2 left-0 -translate-x-full -translate-y-1/2" />
        {conversation ? (
          <div
            data-slot="conversation-toolbar"
            className={`flex h-full min-w-0 ${split ? "w-full md:w-(--conversation-share) md:shrink md:grow-0" : "flex-1"} ${views && layout.mobile === "view" ? "hidden md:flex" : ""} ${layout.conversationFirst ? "order-0" : "order-2"}`}
          >
            <ConversationHeader />
          </div>
        ) : null}
        {split ? (
          <div className="relative order-1 hidden w-(--spacing-workspace-gap) shrink-0 md:block">
            <ChromeSeparator className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2" />
          </div>
        ) : null}
        {views ? (
          <div
            ref={onViewToolbarHost}
            data-slot="view-toolbar-host"
            className={`h-full min-w-0 flex-1 ${layout.mobile === "conversation" && conversation ? "hidden md:block" : ""} ${layout.conversationFirst ? "order-2" : "order-0"}`}
          />
        ) : null}
      </div>
    </header>
  );
}

function HeaderNavigation() {
  const {
    createConversation,
    navigationHistory,
    openTab,
    shell,
    shellState,
    toggleConversation,
    travel,
  } = useClient();
  const layout = useSource(shell);
  const history = useSource(navigationHistory);
  const chat = useChat();
  return (
    <div
      className={`@container/header-navigation flex h-full w-(--spacing-shell-rail) shrink-0 items-center gap-1 px-(--spacing-chrome-rail-action-inset) ${layout.navigation ? "md:w-[calc(var(--navigation-width)+var(--spacing-shell-rail)+var(--spacing-workspace-inset)*3+1px)]" : ""}`}
    >
      <NavigationToggle />
      <span
        className={
          layout.navigation
            ? "hidden items-center gap-1 md:flex @max-[240px]/header-navigation:hidden"
            : "hidden"
        }
      >
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
      <div className={layout.navigation ? "hidden items-center gap-1 md:flex" : "hidden"}>
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
                  toggleConversation();
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
  );
}

function ConversationHeader() {
  const { openTab, shell, shellState, toggleConversation } = useClient();

  const layout = useSource(shell);
  const movable =
    layout.section === "conversations" &&
    layout.tabs.length > 0 &&
    layout.conversationVisible &&
    !layout.expanded;
  return (
    <div className="flex h-full min-w-0 flex-1 items-center">
      <div
        draggable={movable}
        title={movable ? "Drag to move the conversation panel" : undefined}
        onDragStart={(event) => {
          startPanelDrag(event.dataTransfer, "conversation");
        }}
        className="flex min-w-0 flex-1 items-center gap-2 px-2.5 text-chrome md:px-3.5"
      >
        <HeaderTitle />
      </div>
      <div className="flex shrink-0 items-center gap-0.5 pr-1.5">
        {layout.section === "conversations" ? (
          <>
            <IconButton
              icon={layout.tabs.length ? PanelRight : Plus}
              label={layout.tabs.length ? "Toggle conversation panel" : "Open a new view"}
              active={layout.tabs.length > 0 && layout.conversationVisible}
              {...(layout.tabs.length > 0 ? { "aria-pressed": layout.conversationVisible } : {})}
              onPress={() => {
                if (layout.tabs.length > 0) {
                  toggleConversation();
                } else {
                  openTab();
                }
              }}
            />
            <span className="contents">
              <IconButton
                icon={Logs}
                label="Toggle summary"
                active={layout.summary}
                aria-pressed={layout.summary}
                onPress={() => {
                  shellState.update((s) => ({ ...s, summary: !s.summary }));
                }}
              />
            </span>
            <ConversationMenu />
          </>
        ) : null}
      </div>
    </div>
  );
}

function HeaderTitle() {
  const { shell, shellState, shellUi, conversationsUi, navigate } = useClient();
  const layout = useSource(shell);
  const sections = useSource(shellUi.sections);
  const labels = useSource(conversationsUi.header.items);
  const chat = useChat();
  const SectionHeader = sections.find((section) => section.id === layout.section)?.header;
  if (layout.section === "settings") {
    const providerConfiguration =
      layout.settings === "providers" && layout.params.providerAccount !== undefined;
    const title =
      {
        general: "General",
        git: "Git",
        conversation: "Conversation",
        connections: "Connections",
        providers: "Providers",
        plugins: "Plugins",
      }[layout.settings] ?? "General";
    return (
      <>
        <span className="hidden md:inline">Settings</span>
        <div className="min-w-0 md:hidden">
          <Breadcrumbs
            aria-label="Settings path"
            separator={<span aria-hidden="true">/</span>}
            className="min-w-0"
          >
            <Breadcrumbs.Item
              onPress={() => {
                shellState.update((current) => ({ ...current, navigation: true }));
              }}
            >
              Settings
            </Breadcrumbs.Item>
            <Breadcrumbs.Item
              aria-current={providerConfiguration ? false : "page"}
              {...(providerConfiguration
                ? {
                    onPress: () => {
                      navigate({ params: {} });
                    },
                  }
                : {})}
            >
              {title}
            </Breadcrumbs.Item>
            {providerConfiguration ? (
              <Breadcrumbs.Item aria-current="page">Configuration</Breadcrumbs.Item>
            ) : null}
          </Breadcrumbs>
        </div>
      </>
    );
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
          <MessageCircle className={`${headerIconStyle} text-muted`} />
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
      aria-expanded={layout.navigation}
      aria-controls="client-navigation"
      onPress={() => {
        shellState.update((current) => ({ ...current, navigation: !current.navigation }));
      }}
    />
  );
  return layout.section === "settings" ? <span className="md:hidden">{control}</span> : control;
}
