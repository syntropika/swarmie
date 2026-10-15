import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
// @vitest-environment happy-dom
import { render as testingRender, screen, cleanup, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactNode } from "react";
import { afterEach, expect, it, vi } from "vitest";

import { newThread } from "~/web/src/protocol";
import type { Thread } from "~/web/src/protocol";
import { ResourceLibrary, SlashCommands } from "~/web/src/resource-library";
import { deferred } from "~/web/src/test-fixtures";
import { WorkspaceStore } from "~/web/src/workspace";

function render(ui: ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
  return testingRender(ui, {
    wrapper: ({ children }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    ),
  });
}
afterEach(cleanup);

function fixture(harness = "codex") {
  let thread: Thread = {
    ...newThread("project", "/project", harness),
    id: "thread",
    sessionId: "session",
    draft: "/check",
    capabilities: {
      nativeSkillInputs: { availability: "available", source: "harness", evidence: "negotiated" },
    },
    contextItems: [],
  };
  const store = new WorkspaceStore();
  const initial = store.getSnapshot();
  vi.spyOn(store, "getSnapshot").mockImplementation(() => ({
    ...initial,
    threads: [thread],
    extensions: [],
  }));
  const updateThread = vi.spyOn(store, "updateThread").mockImplementation((_id, update) => {
    thread = { ...thread, ...update };
  });
  const rpc = vi.fn();
  const invokeExtension = vi.fn();
  vi.spyOn(store, "rpc").mockImplementation(rpc);
  vi.spyOn(store, "invokeExtension").mockImplementation(invokeExtension);
  const setError = vi.spyOn(store, "setError").mockImplementation(() => {});
  const addContext = vi.spyOn(store, "addContext").mockImplementation(() => {});
  return {
    store,
    api: { rpc, invokeExtension, updateThread, setError, addContext },
    thread,
    current: () => thread,
  };
}

it("keeps two same-name native origins selectable and preserves the chosen exact path", async () => {
  const f = fixture();
  f.api.rpc.mockResolvedValue([
    {
      name: "check",
      source: "skill",
      path: "/project/.agents/skills/check/SKILL.md",
      scope: "project",
      invocation: "$check ",
      availability: "advertised",
    },
    {
      name: "check",
      source: "skill",
      path: "/home/.agents/skills/check/SKILL.md",
      scope: "user",
      invocation: "$check ",
      availability: "advertised",
    },
  ]);
  const setText = vi.fn<(value: string) => void>();
  render(<SlashCommands thread={f.thread} store={f.store} setText={setText} />);
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "$check Native skill · user" }));
  expect(f.current().contextItems?.[0].skill).toEqual({
    name: "check",
    path: "/home/.agents/skills/check/SKILL.md",
  });
  expect(setText).toHaveBeenCalledWith("$check ");
});

it("uses the advertised Claude alias while retaining the original skill name and path", async () => {
  const f = fixture("claude");
  f.api.rpc.mockResolvedValue([
    {
      name: "check",
      source: "skill",
      path: "/canonical/check/SKILL.md",
      invocation: "/swarmie-check-origin ",
      availability: "advertised",
    },
  ]);
  const setText = vi.fn<(value: string) => void>();
  render(<SlashCommands thread={f.thread} store={f.store} setText={setText} />);
  await userEvent
    .setup()
    .click(await screen.findByRole("button", { name: /\$check Native skill/u }));
  expect(f.current().contextItems?.[0].skill).toEqual({
    name: "check",
    path: "/canonical/check/SKILL.md",
  });
  expect(setText).toHaveBeenCalledWith("/swarmie-check-origin ");
});

it("does not offer stale or model-only skills and disables origins without a path", async () => {
  const f = fixture("pi");
  f.api.rpc.mockResolvedValue([
    { name: "check-stale", source: "skill", path: "/stale", availability: "requires-restart" },
    { name: "check-model", source: "skill", path: "/model", userInvocable: false },
    { name: "check-unknown", source: "skill", availability: "origin-unverified" },
  ]);
  render(<SlashCommands thread={f.thread} store={f.store} setText={() => {}} />);
  const unknown = await screen.findByRole("button", { name: /check-unknown/u });
  expect(unknown.hasAttribute("disabled")).toBe(true);
  expect(screen.queryByRole("button", { name: /check-stale|check-model/u })).toBeNull();
});

it("hides the previous node's catalog while an identical session identity reloads on another store", async () => {
  const first = fixture();
  const second = fixture();
  first.api.rpc.mockResolvedValue([
    {
      name: "check-first",
      source: "skill",
      path: "/same/path/SKILL.md",
      availability: "advertised",
    },
  ]);
  const pending = deferred<unknown>();
  second.api.rpc.mockImplementation(async () => await pending.promise);
  const view = render(
    <SlashCommands thread={first.thread} store={first.store} setText={() => {}} />,
  );
  await screen.findByRole("button", { name: /check-first/u });
  view.rerender(<SlashCommands thread={second.thread} store={second.store} setText={() => {}} />);
  expect(screen.queryByRole("button", { name: /check-first/u })).toBeNull();
  await waitFor(() => {
    expect(second.api.rpc).toHaveBeenCalledWith("agent/commands", { sessionId: "session" });
  });
  pending.resolve([
    {
      name: "check-second",
      source: "skill",
      path: "/same/path/SKILL.md",
      availability: "advertised",
    },
  ]);
  await userEvent.setup().click(await screen.findByRole("button", { name: /check-second/u }));
  expect(second.current().contextItems?.[0].skill?.name).toBe("check-second");
  expect(first.current().contextItems).toEqual([]);
});

it("distinguishes attaching discovered instructions from invoking an advertised native skill", async () => {
  const f = fixture("claude");
  f.api.invokeExtension.mockImplementation(async (_extension, command) => {
    if (command === "templates.list") {
      return { templates: [] };
    }
    if (command === "resources.list") {
      return {
        resources: [
          {
            id: "check",
            name: "check",
            kind: "skill",
            source: "project",
            path: "/canonical/SKILL.md",
            native: false,
            availability: "discovered",
          },
        ],
        limitations: [],
      };
    }
    if (command === "resources.read") {
      return { path: "/canonical/SKILL.md", text: "Shared instructions" };
    }
    throw new Error("Unexpected extension command");
  });
  render(<ResourceLibrary thread={f.thread} store={f.store} />);
  const user = userEvent.setup();
  await user.click(
    await screen.findByRole("button", { name: /check skill · project · discovered/u }),
  );
  await waitFor(() => {
    expect(screen.getByRole("button", { name: "Use native skill" }).hasAttribute("disabled")).toBe(
      true,
    );
  });
  await user.click(screen.getByRole("button", { name: "Attach instructions as text" }));
  expect(f.api.addContext).toHaveBeenCalledWith(
    "thread",
    expect.objectContaining({ text: "Shared instructions", path: "/canonical/SKILL.md" }),
  );
  expect(f.current().contextItems).toEqual([]);
});

it("uses the advertised native binding when its name and path differ from the canonical preview", async () => {
  const f = fixture();
  f.api.invokeExtension.mockImplementation(async (_extension, command) => {
    if (command === "templates.list") {
      return { templates: [] };
    }
    if (command === "resources.list") {
      return {
        resources: [
          {
            id: "check",
            name: "check",
            kind: "skill",
            source: "user",
            path: "/canonical/SKILL.md",
            native: true,
            availability: "advertised",
            invocation: "$plugin:check ",
            selection: { name: "plugin:check", path: "/native/alias/SKILL.md" },
          },
        ],
        limitations: [],
      };
    }
    if (command === "resources.read") {
      return { path: "/canonical/SKILL.md", text: "Shared instructions" };
    }
    throw new Error("Unexpected extension command");
  });
  render(<ResourceLibrary thread={f.thread} store={f.store} />);
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: /check skill · user · advertised/u }));
  await user.click(await screen.findByRole("button", { name: "Use native skill" }));
  expect(f.current().contextItems?.[0].skill).toEqual({
    name: "plugin:check",
    path: "/native/alias/SKILL.md",
  });
});
