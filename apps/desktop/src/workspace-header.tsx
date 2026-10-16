import { Button, Dropdown, Input } from "@swarmie/ui-web";
import {
  ArrowLeft,
  ArrowRight,
  PanelLeft,
  Settings,
  Folder,
  PanelRight,
  GitBranch,
  MoreHorizontal,
  Pencil,
  PinOff,
  Pin,
  Archive,
  Check,
  History,
  Search,
  Command,
} from "lucide-react";

// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import { startAction } from "~/src/action-boundary";
import { BackgroundTasks } from "~/src/background-tasks";
import { ContributionSlot, ContributionNavigation } from "~/src/contribution-host";
import { hasNumber } from "~/src/value-presence";
import { IconButton } from "~/src/workspace-icon-button";
import type { WorkspaceViewState } from "~/src/workspace-state";
import { blank } from "~/src/workspace-state";

export function WorkspaceHeader({
  view,
}: {
  view: Pick<
    WorkspaceViewState,
    | "store"
    | "state"
    | "setSidebar"
    | "panel"
    | "setPanel"
    | "setPalette"
    | "setFinder"
    | "setUndo"
    | "renaming"
    | "setRenaming"
    | "title"
    | "setTitle"
    | "navigation"
    | "active"
    | "project"
    | "perform"
    | "navigate"
    | "renameActive"
    | "archiveDisabled"
    | "archiveActive"
    | "pinActive"
  >;
}) {
  const {
    store,
    state,
    setSidebar,
    panel,
    setPanel,
    setPalette,
    setFinder,
    setUndo,
    renaming,
    setRenaming,
    title,
    setTitle,
    navigation,
    active,
    project,
    perform,
    navigate,
    renameActive,
    archiveDisabled,
    archiveActive,
    pinActive,
  } = view;
  return (
    <header className="titlebar">
      <div className="titlebar-navigation">
        <IconButton
          label="Back"
          onClick={() => {
            navigate(-1);
          }}
          disabled={navigation.index < 1}
        >
          <ArrowLeft size={14} />
        </IconButton>
        <IconButton
          label="Forward"
          onClick={() => {
            navigate(1);
          }}
          disabled={navigation.index >= navigation.ids.length - 1}
        >
          <ArrowRight size={14} />
        </IconButton>
        <IconButton
          label="Toggle sidebar"
          sidebarToggle
          onClick={() => {
            if (window.innerWidth <= 1100 && panel) {
              setPanel(null);
            }
            setSidebar((v) => !v);
          }}
        >
          <PanelLeft size={15} />
        </IconButton>
      </div>
      <div className="titlebar-topic">
        {panel === "settings" ? <Settings size={14} /> : <Folder size={14} />}
        {renaming ? (
          <form
            onSubmit={(e) => {
              e.preventDefault();
              startAction(
                perform(async () => {
                  await store.rename(active.id, title);
                  setRenaming(false);
                }),
                store.setError.bind(store),
              );
            }}
          >
            <Input
              aria-label="Conversation title"
              autoFocus
              value={title}
              onChange={(e) => {
                setTitle(e.target.value);
              }}
              onBlur={() => {
                startAction(
                  perform(async () => {
                    await store.rename(active.id, title);
                  }),
                  store.setError.bind(store),
                );
                setRenaming(false);
              }}
            />
          </form>
        ) : (
          <h1>
            {(() => {
              if (panel === "settings") {
                return "Settings";
              }
              if (active.title === "New chat" && project) {
                return project.name;
              }
              return active.title;
            })()}
          </h1>
        )}
      </div>
      <div className="titlebar-actions">
        <ContributionSlot name="app.header.actions" />
        <ContributionNavigation kind="screen" />
        <ContributionNavigation kind="panel" />
        <BackgroundTasks store={store} thread={active} connected={state.connected} />
        <IconButton
          label="Workspace tools"
          active={panel === "extensions"}
          disabled={!state.extensions.some((e) => hasNumber(e.views?.length))}
          onClick={() => {
            setPanel(panel === "extensions" ? null : "extensions");
          }}
        >
          <PanelRight size={16} />
        </IconButton>
        <IconButton
          label="Review changes"
          active={panel === "review"}
          disabled={!(project?.git === true)}
          onClick={() => {
            setPanel(panel === "review" ? null : "review");
            startAction(
              perform(async () => {
                await store.review();
              }),
              store.setError.bind(store),
            );
          }}
        >
          <GitBranch size={16} />
        </IconButton>
        <Dropdown>
          <Button
            variant="tertiary"
            size="md"
            isIconOnly

            aria-label="Conversation actions"
          >
            <MoreHorizontal size={17} />
          </Button>
          <Dropdown.Popover placement="bottom end">
            <Dropdown.Menu aria-label="Conversation actions">
              <ContributionSlot name="conversation.contextMenu" menu />
              <Dropdown.Item
                id="rename"
                textValue="Rename chat"
                isDisabled={active === blank}
                onAction={renameActive}
              >
                <Pencil size={14} />
                Rename chat
              </Dropdown.Item>
              <Dropdown.Item
                id="pin"
                textValue={active.pinned ? "Unpin chat" : "Pin chat"}
                isDisabled={active === blank}
                onAction={pinActive}
              >
                {active.pinned ? <PinOff size={14} /> : <Pin size={14} />}
                {active.pinned ? "Unpin chat" : "Pin chat"}
              </Dropdown.Item>
              <Dropdown.Item
                id="archive"
                textValue={active.archived === true ? "Restore chat" : "Archive chat"}
                isDisabled={archiveDisabled}
                onAction={archiveActive}
              >
                <Archive size={14} />
                {active.archived === true ? "Restore chat" : "Archive chat"}
              </Dropdown.Item>
              <Dropdown.Item
                id="settle"
                textValue={active.settled === true ? "Reopen conversation" : "Mark settled"}
                isDisabled={active === blank}
                onAction={() => {
                  const previous = { settled: active.settled, snoozeUntil: active.snoozeUntil };
                  store.updateThread(active.id, { settled: !(active.settled === true) });
                  setUndo({
                    title: "Conversation updated",
                    run: () => {
                      store.updateThread(active.id, previous);
                      return undefined;
                    },
                  });
                }}
              >
                <Check size={14} />
                {active.settled === true ? "Reopen conversation" : "Mark settled"}
              </Dropdown.Item>
              <Dropdown.Item
                id="snooze"
                textValue={hasNumber(active.snoozeUntil) ? "Unsnooze" : "Snooze for one hour"}
                isDisabled={active === blank}
                onAction={() => {
                  const previous = active.snoozeUntil;
                  store.updateThread(active.id, {
                    snoozeUntil: hasNumber(active.snoozeUntil) ? undefined : Date.now() + 3_600_000,
                  });
                  setUndo({
                    title: "Snooze updated",
                    run: () => {
                      store.updateThread(active.id, { snoozeUntil: previous });
                      return undefined;
                    },
                  });
                }}
              >
                <History size={14} />
                {hasNumber(active.snoozeUntil) ? "Unsnooze" : "Snooze for one hour"}
              </Dropdown.Item>
              <Dropdown.Item
                id="files"
                textValue="Find file"
                onAction={() => {
                  setFinder(true);
                }}
              >
                <Search size={14} />
                Find file
              </Dropdown.Item>
              <Dropdown.Item
                id="commands"
                textValue="Command palette"
                onAction={() => {
                  setPalette(true);
                }}
              >
                <Command size={14} />
                Command palette
              </Dropdown.Item>
            </Dropdown.Menu>
          </Dropdown.Popover>
        </Dropdown>
      </div>
    </header>
  );
}
