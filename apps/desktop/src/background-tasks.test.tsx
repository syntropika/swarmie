import { cleanup, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";

import { BackgroundTasks } from "~/src/background-tasks";
import { newThread } from "~/src/protocol";
import type { AgentTask, Thread } from "~/src/protocol";
import { deferred, fixtureControl, renderWithQuery as render } from "~/src/test-fixtures";
import { WorkspaceStore } from "~/src/workspace";

const node = {
  environmentId: "node",
  storeId: "store",
  ownerScopeId: "owner",
};
const task: AgentTask = {
  id: "native-task",
  description: "Inspect the build output",
  status: "running",
};
async function queuedResponse(resolves: ((value: unknown) => void)[]) {
  const response = deferred<unknown>();
  resolves.push(response.resolve);
  return await response.promise;
}
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  localStorage.clear();
});
async function fixture() {
  const store = new WorkspaceStore();
  await store.ready();
  const thread: Thread = {
    ...newThread("project", "/fixture", "claude"),
    sessionId: "native-session",
    state: "running",
    capabilities: {
      backgroundTasks: {
        availability: "available",
        source: "harness",
        evidence: "declared",
      },
    },
  };
  Object.assign(store.getSnapshot(), {
    node,
    connected: true,
    threads: [thread],
    activeId: thread.id,
    harnesses: [
      {
        harness: "codex",
        available: true,
      },
      {
        harness: "cursor",
        available: true,
      },
    ],
    disabledHarnesses: [],
  });
  const calls: {
    method: string;
    args: unknown;
  }[] = [];
  const rpc = vi.spyOn(store, "request").mockImplementation(async (method, args) => {
    calls.push({
      method,
      args,
    });
    if (method === "session/tasks") {
      return [task];
    }
    if (method === "session/task/stop") {
      return null;
    }
    throw new Error("Unexpected fixture method");
  });
  return {
    store,
    thread,
    calls,
    rpc,
  };
}
it("lists and stops a task during a running parent turn without cancelling or creating a session", async () => {
  const { store, thread, calls } = await fixture();
  render(<BackgroundTasks store={store} thread={thread} connected />);
  await userEvent.click(
    screen.getByRole("button", {
      name: "Background tasks",
    }),
  );
  await screen.findByText(task.description);
  await userEvent.click(
    screen.getByRole("button", {
      name: `Stop ${task.description}`,
    }),
  );
  expect(await screen.findByText("Stop requested · waiting for confirmation")).toBeTruthy();
  expect(calls.find((call) => call.method === "session/task/stop")?.args).toEqual({
    sessionId: "native-session",
    taskId: "native-task",
  });
  expect(
    calls.some((call) =>
      ["session/cancel", "session/create", "session/prompt"].includes(call.method),
    ),
  ).toBe(false);
  expect(store.getSnapshot().threads[0].state).toBe("running");
});
it("keeps an accepted stop request across closing and reopening the same task view", async () => {
  const { store, thread, rpc } = await fixture();
  const response = deferred<unknown>();
  const { resolve } = response;
  rpc.mockImplementation(async (method) =>
    method === "session/tasks" ? [task] : await response.promise,
  );
  render(<BackgroundTasks store={store} thread={thread} connected />);
  await userEvent.click(
    screen.getByRole("button", {
      name: "Background tasks",
    }),
  );
  await userEvent.click(
    await screen.findByRole("button", {
      name: `Stop ${task.description}`,
    }),
  );
  await userEvent.tab();
  await userEvent.keyboard("{Escape}");
  await waitFor(() => {
    expect(screen.queryByRole("dialog", { name: "Background tasks" })).toBeNull();
  });
  resolve(null);
  await userEvent.click(
    screen.getByRole("button", {
      name: "Background tasks",
    }),
  );
  expect(await screen.findByText("Stop requested · waiting for confirmation")).toBeTruthy();
});
it("discards a previous chat view's stop receipt when returning to that chat", async () => {
  const { store, thread, rpc } = await fixture();
  const other = {
    ...thread,
    id: "other-chat",
    sessionId: "other-session",
  };
  store.getSnapshot().threads.push(other);
  const resolves: ((value: unknown) => void)[] = [];
  rpc.mockImplementation(async (method) =>
    method === "session/tasks" ? [task] : await queuedResponse(resolves),
  );
  const view = render(<BackgroundTasks store={store} thread={thread} connected />);
  await userEvent.click(
    screen.getByRole("button", {
      name: "Background tasks",
    }),
  );
  await userEvent.click(
    await screen.findByRole("button", {
      name: `Stop ${task.description}`,
    }),
  );
  view.rerender(<BackgroundTasks store={store} thread={other} connected />);
  view.rerender(<BackgroundTasks store={store} thread={thread} connected />);
  await userEvent.click(
    screen.getByRole("button", {
      name: "Background tasks",
    }),
  );
  await userEvent.click(
    await screen.findByRole("button", {
      name: `Stop ${task.description}`,
    }),
  );
  resolves[0](null);
  await waitFor(() => {
    expect(screen.queryByText("Stop requested · waiting for confirmation")).toBeNull();
  });
  expect(
    fixtureControl(
      screen.getByRole("button", {
        name: `Stop ${task.description}`,
      }),
    ).disabled,
  ).toBe(true);
  resolves[1](null);
  expect(await screen.findByText("Stop requested · waiting for confirmation")).toBeTruthy();
});
it("hides unsupported task controls and ignores an earlier node's delayed task list", async () => {
  const { store, thread, rpc, calls } = await fixture();
  const view = render(
    <BackgroundTasks
      store={store}
      thread={{
        ...thread,
        capabilities: {},
      }}
      connected
    />,
  );
  expect(
    screen.queryByRole("button", {
      name: "Background tasks",
    }),
  ).toBeNull();
  expect(calls).toHaveLength(0);
  const response = deferred<unknown>();
  const { resolve } = response;
  rpc.mockImplementation(async () => await response.promise);
  view.rerender(<BackgroundTasks store={store} thread={thread} connected />);
  await userEvent.click(
    screen.getByRole("button", {
      name: "Background tasks",
    }),
  );
  Object.assign(store.getSnapshot(), {
    node: {
      ...node,
      environmentId: "other-node",
    },
  });
  view.rerender(<BackgroundTasks store={store} thread={thread} connected />);
  resolve([task]);
  await waitFor(() => {
    expect(screen.queryByText(task.description)).toBeNull();
  });
});
