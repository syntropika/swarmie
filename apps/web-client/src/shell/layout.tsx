import { useSource } from "@swarmie/client-ui";
import { OnboardingGuide } from "@swarmie/plugin-browser/onboarding/views";
import { ProjectsContext } from "@swarmie/plugin-projects-browser/views";
import { ProvidersPanel } from "@swarmie/plugin-providers-browser/views";
import { ChromeButton } from "@swarmie/ui-web";
import { Globe, MessageCircle } from "lucide-react";
import { useLayoutEffect, useRef, useState } from "react";
import type { ReactElement } from "react";

import { useClient } from "../client/context";
import { iconStyle, useChat } from "../controls";
import { ConversationPanel } from "../conversations/conversation";
import { SettingsPage } from "../settings/page";
import { ViewPanel } from "../views/panel";
import { Header } from "./header";
import { Navigation } from "./navigation/navigation";
import { draggedPanel } from "./panel-drag";
import { PanelDropOverlay } from "./panel-drop";
import { PinnedContext } from "./pinned-context";
import { ResizeHandle } from "./resize";

function ApplicationGuide() {
  const { onboarding, providers } = useClient();
  const guide = useSource(onboarding);
  return (
    <>
      {" "}
      {guide.open ? (
        <OnboardingGuide
          onComplete={() => {
            onboarding.update(() => ({ completed: true, open: false }));
          }}
          renderProviderConfiguration={(events) => (
            <ProvidersPanel
              providers={providers}
              onConfigured={events.onConfigured}
              onSelectionChange={(choice) => {
                events.onSelectionChange(choice !== null);
              }}
            />
          )}
        />
      ) : null}
    </>
  );
}
function MobilePanels() {
  const { shell, views: viewState, showConversation } = useClient();
  const layout = useSource(shell);
  const views = layout.tabs.length > 0;
  return (
    <>
      {" "}
      {layout.section === "conversations" && views ? (
        <nav
          aria-label="Mobile panels"
          className="flex h-(--spacing-panel-switch) shrink-0 bg-chrome-surface md:hidden"
        >
          <ChromeButton
            size="panelSwitch"
            selected={layout.mobile === "view"}
            aria-pressed={layout.mobile === "view"}
            onPress={() => {
              viewState.update((s) => ({ ...s, mobile: "view" }));
            }}
          >
            <Globe className={iconStyle} />
            View
          </ChromeButton>
          <ChromeButton
            size="panelSwitch"
            selected={layout.mobile === "conversation"}
            aria-pressed={layout.mobile === "conversation"}
            onPress={showConversation}
          >
            <MessageCircle className={iconStyle} />
            Conversation
          </ChromeButton>
        </nav>
      ) : null}
    </>
  );
}
export function App() {
  const client = useClient();
  return (
    <ProjectsContext value={client.projectsPresentation}>
      <ApplicationLayout />
    </ProjectsContext>
  );
}
function ApplicationLayout() {
  const chat = useChat();
  const { shell, shellUi, views: viewState } = useClient();
  const sections = useSource(shellUi.sections);
  const layout = useSource(shell);

  const [viewToolbarHost, setViewToolbarHost] = useState<HTMLDivElement | null>(null);
  const [dragging, setDragging] = useState<"conversation" | "view" | null>(null);
  const views = layout.tabs.length > 0;
  const viewArea = useRef<HTMLDivElement>(null);
  const panels = useRef<HTMLElement>(null);
  const focusedViewElement = useRef<Element | null>(null);
  const hadViews = useRef(views);
  useLayoutEffect(() => {
    const closedLastView = hadViews.current && !views;
    hadViews.current = views;
    if (
      closedLastView &&
      focusedViewElement.current !== null &&
      !focusedViewElement.current.isConnected &&
      document.activeElement === document.body
    ) {
      const input = panels.current?.querySelector<HTMLTextAreaElement>(
        'section[aria-label="Conversation"] textarea',
      );
      if (input && input.getClientRects().length > 0) {
        input.focus();
      }
    }
  }, [views]);
  const showConversation = !views || layout.conversationVisible;
  function dropPanel(side: "left" | "right", event: React.DragEvent) {
    const panel = draggedPanel(event.dataTransfer);
    if (panel === "conversation" || panel === "view") {
      viewState.update((s) => ({
        ...s,
        conversationFirst: panel === "conversation" ? side === "left" : side === "right",
      }));
    }
    setDragging(null);
  }
  return (
    <div
      onFocusCapture={(event) => {
        focusedViewElement.current =
          (viewArea.current?.contains(event.target) ?? false) ||
          event.target.closest('[data-slot="view-toolbar"]') !== null
            ? event.target
            : null;
      }}
      onBlurCapture={(event) => {
        if (event.relatedTarget === null && focusedViewElement.current?.isConnected === true) {
          focusedViewElement.current = null;
        }
      }}
      onDragStart={(event) => {
        setDragging(draggedPanel(event.dataTransfer));
      }}
      onDragEnd={() => {
        setDragging(null);
      }}
      className="bg-client-page flex h-dvh flex-col overflow-hidden pr-(--spacing-shell-frame) pb-(--spacing-shell-bottom) font-sans text-foreground md:bg-workspace md:pb-(--spacing-shell-frame)"
    >
      <ApplicationGuide />
      {sections.map(({ id, overlay: Overlay }) => (Overlay ? <Overlay key={id} /> : null))}
      <Header onViewToolbarHost={setViewToolbarHost} />
      <div className="flex min-h-0 flex-1 overflow-hidden bg-workspace p-(--spacing-workspace-inset)">
        <Navigation />
        <main
          ref={panels}
          id="client-panels"
          className={`relative flex min-h-0 min-w-0 flex-1 overflow-hidden rounded-t-(--radius-workspace-upper) rounded-b-none bg-workspace md:rounded-br-(--radius-shell-frame) ${layout.navigation ? "md:rounded-l-none" : "md:rounded-bl-(--radius-shell-frame)"}`}
        >
          <SectionContent>
            <>
              {showConversation ? (
                <div
                  className={`min-h-0 min-w-0 ${views && !layout.expanded ? "w-full md:w-(--conversation-share) md:shrink md:grow-0" : "flex-1"} ${views && layout.mobile === "view" ? "hidden md:block" : ""} ${layout.conversationFirst ? "order-0" : "order-2"}`}
                >
                  <ConversationPanel chat={chat} />
                </div>
              ) : null}
              {views && showConversation ? (
                <div className="order-1 hidden md:flex">
                  <ResizeHandle target="panels" />
                </div>
              ) : null}
              {views ? (
                <div
                  ref={viewArea}
                  className={`min-h-0 min-w-0 flex-1 ${layout.mobile === "conversation" && showConversation ? "hidden md:block" : ""} ${layout.conversationFirst ? "order-2" : "order-0"}`}
                >
                  <ViewPanel toolbarHost={viewToolbarHost} />
                </div>
              ) : null}
              <PinnedContext />
              {views && showConversation && !layout.expanded && dragging !== null ? (
                <PanelDropOverlay panel={dragging} onDrop={dropPanel} />
              ) : null}
            </>
          </SectionContent>
        </main>
      </div>
      <MobilePanels />
    </div>
  );
}

function SectionContent({ children }: { children: ReactElement }) {
  const { shell, shellUi } = useClient();
  const layout = useSource(shell);
  const sections = useSource(shellUi.sections);
  const Screen = sections.find((section) => section.id === layout.section)?.component;
  if (layout.section === "settings") {
    return <SettingsPage />;
  }
  if (Screen) {
    return (
      <section className="min-h-0 min-w-0 flex-1 overflow-auto bg-pane shadow-sheet">
        <Screen params={layout.params} />
      </section>
    );
  }
  return children;
}
