// @vitest-environment happy-dom
import { cleanup, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, expect, it, vi } from "vitest";

import { NodeClient } from "~/src/nodes";
import { RenameChatDialog } from "~/src/rename-chat-dialog";
import { SettingsPage } from "~/src/settings-page";
import type { SettingsSection } from "~/src/settings-sections";
import { defaultShortcuts } from "~/src/shortcuts";
import { deferred, renderWithQuery as render } from "~/src/test-fixtures";
import { WorkspaceStore } from "~/src/workspace";
import { WorkspaceSearch } from "~/src/workspace-search";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

async function workspace() {
  localStorage.clear();
  const store = new WorkspaceStore();
  await store.ready();
  store.createThread();
  const thread = store.active();
  if (!thread) {
    throw new Error("Expected a chat fixture.");
  }
  return { store, thread };
}

function noAction() {}

function SettingsSearchFixture({ store, client }: { store: WorkspaceStore; client: NodeClient }) {
  const [selectedSection, setSelectedSection] = useState<SettingsSection>("general");
  const [searchOpen, setSearchOpen] = useState(true);
  return (
    <>
      <button
        type="button"
        onClick={() => {
          setSearchOpen(true);
        }}
      >
        Search settings destinations
      </button>
      <SettingsPage
        section={selectedSection}
        onSectionChange={setSelectedSection}
        store={store}
        state={store.getSnapshot()}
        client={client}
        nodes={client.getSnapshot()}
        shortcuts={defaultShortcuts}
        setShortcuts={noAction}
        onClose={noAction}
        onSetup={noAction}
      />
      {searchOpen && (
        <WorkspaceSearch
          store={store}
          onSettings={setSelectedSection}
          onPanel={noAction}
          onOpenProject={noAction}
          onClose={() => {
            setSearchOpen(false);
          }}
        />
      )}
    </>
  );
}

it("reopens the same settings search destination after navigating in the settings sidebar", async () => {
  const { store } = await workspace();
  render(<SettingsSearchFixture store={store} client={new NodeClient(store)} />);
  await userEvent.type(
    screen.getByRole("combobox", { name: "Search workspace" }),
    "Keyboard shortcuts",
  );
  await userEvent.keyboard("{Enter}");
  expect(screen.getByRole("heading", { level: 1, name: "Keyboard shortcuts" })).toBeTruthy();
  await userEvent.click(screen.getByRole("button", { name: "Notifications" }));
  expect(screen.getByRole("heading", { level: 1, name: "Notifications" })).toBeTruthy();
  await userEvent.click(screen.getByRole("button", { name: "Search settings destinations" }));
  await userEvent.type(
    screen.getByRole("combobox", { name: "Search workspace" }),
    "Keyboard shortcuts",
  );
  await userEvent.keyboard("{Enter}");
  expect(screen.getByRole("heading", { level: 1, name: "Keyboard shortcuts" })).toBeTruthy();
});

function SearchFixture({
  store,
  scope = "all",
  close = noAction,
  select = noAction,
}: {
  store: WorkspaceStore;
  scope?: "all" | "chats" | "files" | "commands";
  close?: () => void;
  select?: (id: string) => void;
}) {
  return (
    <WorkspaceSearch
      store={store}
      initialScope={scope}
      onClose={close}
      onSelectThread={select}
      onOpenProject={() => {}}
      onPanel={() => {}}
    />
  );
}

it("searches message contents in the unified menu and opens the matching chat with the keyboard", async () => {
  const { store, thread } = await workspace();
  store.updateThread(thread.id, {
    title: "First chat",
    messages: [{ id: "message", role: "user", content: "Check deployment logs" }],
  });
  const select = vi.fn<(id: string) => void>();
  const close = vi.fn<() => void>();
  render(<SearchFixture store={store} select={select} close={close} />);
  expect(screen.getByRole("dialog", { name: "Search workspace" })).toBeTruthy();
  await userEvent.type(screen.getByRole("combobox", { name: "Search workspace" }), "deployment");
  expect(screen.getAllByRole("option")).toHaveLength(1);
  expect(screen.getByText("Check deployment logs")).toBeTruthy();
  await userEvent.keyboard("{Enter}");
  expect(select).toHaveBeenCalledExactlyOnceWith(thread.id);
  expect(close).toHaveBeenCalledOnce();
});

it("keeps renaming explicit, rejects blank names and saves the captured chat once", async () => {
  const { store, thread } = await workspace();
  const rename = vi.spyOn(store, "rename").mockResolvedValue();
  const close = vi.fn<() => void>();
  const view = render(<RenameChatDialog thread={thread} store={store} onClose={close} />);
  const input = screen.getByRole("textbox", { name: "Chat name" });
  await userEvent.clear(input);
  await userEvent.type(input, "Revised name");
  await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(rename).not.toHaveBeenCalled();
  expect(close).toHaveBeenCalledOnce();
  view.unmount();
  close.mockClear();
  render(<RenameChatDialog thread={thread} store={store} onClose={close} />);
  const freshInput = screen.getByRole("textbox", { name: "Chat name" });
  await userEvent.clear(freshInput);
  expect(screen.getByRole<HTMLButtonElement>("button", { name: "Save" }).disabled).toBe(true);
  await userEvent.type(freshInput, "  Revised name  ");
  await userEvent.keyboard("{Enter}");
  await waitFor(() => {
    expect(rename).toHaveBeenCalledExactlyOnceWith(thread.id, "Revised name");
  });
  expect(close).toHaveBeenCalledOnce();
});

it("starts in chats, switches to all results without dismissing and navigates between groups", async () => {
  const { store, thread } = await workspace();
  store.updateThread(thread.id, {
    title: "Pinned chat",
    pinned: true,
    messages: [{ id: "one", role: "user", content: "hello" }],
  });
  const close = vi.fn<() => void>();
  render(<SearchFixture store={store} scope="chats" close={close} />);
  expect(screen.getByRole("combobox", { name: "Search chats" })).toBeTruthy();
  expect(screen.queryByText("Commands", { exact: true })).toBeNull();
  await userEvent.click(screen.getByRole("button", { name: "All results" }));
  await waitFor(() => {
    expect(document.activeElement).toBe(screen.getByRole("combobox", { name: "Search workspace" }));
  });
  expect(screen.getAllByRole("dialog")).toHaveLength(1);
  expect(screen.getByRole("group", { name: "Commands" })).toBeTruthy();
  expect(close).not.toHaveBeenCalled();
  await userEvent.keyboard("{Tab}");
  await waitFor(() => {
    expect(screen.getByRole("option", { name: /Pinned chat/u }).getAttribute("aria-selected")).toBe(
      "true",
    );
  });
  await userEvent.keyboard("{Shift>}{Tab}{/Shift}{Enter}");
  expect(screen.getByRole("combobox", { name: "Search chats" })).toBeTruthy();
  await userEvent.keyboard("{Backspace}");
  expect(screen.getByRole("combobox", { name: "Search workspace" })).toBeTruthy();
  expect(close).not.toHaveBeenCalled();
});

it("creates a chat from the empty scoped search and closes once", async () => {
  const { store } = await workspace();
  const close = vi.fn<() => void>();
  const create = vi.spyOn(store, "createThread");
  render(<SearchFixture store={store} scope="chats" close={close} />);
  expect(screen.getByText("No chats to search yet.")).toBeTruthy();
  await userEvent.click(screen.getByRole("button", { name: "New chat" }));
  expect(create).toHaveBeenCalledOnce();
  expect(close).toHaveBeenCalledOnce();
});

it("keeps the rename dialog open when saving fails", async () => {
  const { store, thread } = await workspace();
  vi.spyOn(store, "rename").mockRejectedValue(new Error("Could not rename this chat"));
  const close = vi.fn<() => void>();
  render(<RenameChatDialog thread={thread} store={store} onClose={close} />);
  await userEvent.click(screen.getByRole("button", { name: "Save" }));
  expect(await screen.findByRole("alert")).toHaveProperty(
    "textContent",
    "Could not rename this chat",
  );
  expect(close).not.toHaveBeenCalled();
});

it("uses runtime file results and opens the selected path", async () => {
  const { store, thread } = await workspace();
  store.updateThread(thread.id, { worktreeId: "project" });
  const find = vi.spyOn(store, "findFiles").mockResolvedValue(["src/main.ts"]);
  const open = vi.spyOn(store, "openFile").mockImplementation(() => {});
  const close = vi.fn<() => void>();
  render(<SearchFixture store={store} scope="files" close={close} />);
  await userEvent.type(screen.getByRole("combobox", { name: "Find file" }), "main");
  await waitFor(() => {
    expect(find).toHaveBeenLastCalledWith("main");
  });
  await userEvent.click(await screen.findByRole("option", { name: "src/main.ts" }));
  expect(open).toHaveBeenCalledExactlyOnceWith({ path: "src/main.ts" });
  expect(close).toHaveBeenCalledOnce();
});

it("keeps stale file responses and errors out of newer searches", async () => {
  const { store, thread } = await workspace();
  store.updateThread(thread.id, { worktreeId: "project" });
  const old = deferred<string[]>();
  const find = vi.spyOn(store, "findFiles").mockImplementation(async (query) => {
    if (query === "old") {
      return await old.promise;
    }
    return query === "new" ? ["new.ts"] : [];
  });
  render(<SearchFixture store={store} scope="files" />);
  const input = screen.getByRole("combobox", { name: "Find file" });
  await userEvent.type(input, "old");
  await waitFor(() => {
    expect(find).toHaveBeenCalledWith("old");
  });
  await userEvent.clear(input);
  await userEvent.type(input, "new");
  expect(screen.queryByRole("option")).toBeNull();
  expect(await screen.findByRole("option", { name: "new.ts" })).toBeTruthy();
  old.reject(new Error("Old query failed"));
  await userEvent.click(input);
  expect(screen.queryByRole("alert")).toBeNull();
  expect(screen.getByRole("option", { name: "new.ts" })).toBeTruthy();
});

it("keeps file failures recoverable within the shared search", async () => {
  const { store, thread } = await workspace();
  store.updateThread(thread.id, { worktreeId: "project" });
  vi.spyOn(store, "findFiles")
    .mockRejectedValueOnce(new Error("Files unavailable"))
    .mockResolvedValue(["recovered.ts"]);
  const close = vi.fn<() => void>();
  render(<SearchFixture store={store} scope="files" close={close} />);
  expect(await screen.findByRole("alert")).toHaveProperty("textContent", "Files unavailable");
  await userEvent.type(screen.getByRole("combobox", { name: "Find file" }), "recovered");
  expect(await screen.findByRole("option", { name: "recovered.ts" })).toBeTruthy();
  expect(screen.queryByRole("alert")).toBeNull();
  expect(close).not.toHaveBeenCalled();
});

it("does not run the selected item when clearing or closing with the keyboard", async () => {
  const { store } = await workspace();
  const create = vi.spyOn(store, "createThread");
  const close = vi.fn<() => void>();
  render(<SearchFixture store={store} close={close} />);
  await userEvent.type(screen.getByRole("combobox", { name: "Search workspace" }), "New chat");
  await userEvent.tab();
  await userEvent.keyboard("{Enter}");
  expect(create).not.toHaveBeenCalled();
  expect(screen.getByRole<HTMLInputElement>("combobox").value).toBe("");
  await userEvent.keyboard("{Escape}");
  expect(close).toHaveBeenCalledOnce();
});

it("uses the settings navigation catalog and hides account security when signed out", async () => {
  const { store } = await workspace();
  const settings = vi.fn<() => void>();
  const panel = vi.fn<() => void>();
  const close = vi.fn<() => void>();
  render(
    <WorkspaceSearch
      store={store}
      onSettings={settings}
      onPanel={panel}
      onOpenProject={noAction}
      onClose={close}
    />,
  );
  expect(screen.queryByRole("option", { name: "Account security" })).toBeNull();
  await userEvent.type(screen.getByRole("combobox", { name: "Search workspace" }), "Notifications");
  expect(screen.getAllByRole("option")).toHaveLength(1);
  await userEvent.keyboard("{Enter}");
  expect(settings).toHaveBeenCalledExactlyOnceWith("notifications");
  expect(panel).toHaveBeenCalledExactlyOnceWith("settings");
  expect(close).toHaveBeenCalledOnce();
});
