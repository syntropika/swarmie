import { cleanup, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";

import { ContextUsage } from "./context-usage";
import { HarnessSelect } from "./harness-select";
import { newThread } from "./protocol";
import type { Thread } from "./protocol";
import { SessionControls } from "./session-controls";
import { sessionPermissions } from "./session-permissions";
import { fixtureControl, renderWithQuery as render } from "./test-fixtures";
import { WorkspaceStore } from "./workspace";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});
it("shows status on harness hover or focus and opens its selector on click", async () => {
  const props = {
    label: "Agent tool",
    value: "codex",
    appearance: "menu" as const,
    onChange: vi.fn<(...args: unknown[]) => void>(),
    harnesses: [
      {
        harness: "codex",
        available: true,
      },
    ],
  };
  const view = render(
    <HarnessSelect
      {...props}
      status={{
        state: "needs_input",
        connected: true,
      }}
    />,
  );
  screen
    .getByRole("button", {
      name: "Agent tool: Codex",
    })
    .focus();
  await waitFor(() => {
    expect(screen.getByRole("tooltip").textContent).toContain(
      "Waiting for your answer or approval",
    );
  });
  for (const label of ["Idle", "Running", "Cancelling", "Failed", "Closed / Disconnected"]) {
    expect(screen.getByRole("tooltip").textContent).toContain(label);
  }
  await userEvent.click(
    screen.getByRole("button", {
      name: "Agent tool: Codex",
    }),
  );
  expect(
    screen.getByRole("menu", {
      name: "Agent tool",
    }),
  ).toBeTruthy();
  await userEvent.keyboard("{Escape}");
  view.rerender(
    <HarnessSelect
      {...props}
      status={{
        state: "running",
        connected: false,
      }}
    />,
  );
  screen
    .getByRole("button", {
      name: "Agent tool: Codex",
    })
    .focus();
  await waitFor(() => {
    expect(screen.getByRole("tooltip").textContent).toContain(
      "The local workspace is not connected",
    );
  });
});
it("shows context occupancy from real token counts and handles unknown capacity", async () => {
  const view = render(
    <ContextUsage
      context={{
        usedTokens: 205_000,
        contextWindowTokens: 258_000,
      }}
    />,
  );
  screen
    .getByRole("button", {
      name: "Context window: 79% full",
    })
    .focus();
  await waitFor(() => {
    expect(screen.getByRole("tooltip").textContent).toContain("205k / 258k tokens used");
  });
  view.rerender(
    <ContextUsage
      context={{
        usedTokens: 0,
        contextWindowTokens: 0,
      }}
    />,
  );
  expect(
    screen.getByRole("button", {
      name: "Context window: usage unavailable",
    }),
  ).toBeTruthy();
  view.rerender(
    <ContextUsage
      context={{
        usedTokens: 0,
        contextWindowTokens: 200_000,
      }}
    />,
  );
  expect(
    screen.getByRole("button", {
      name: "Context window: 0% full",
    }),
  ).toBeTruthy();
  view.rerender(<ContextUsage context={undefined} />);
  expect(
    screen.getByRole("button", {
      name: "Context window: usage unavailable",
    }),
  ).toBeTruthy();
});
it("leaves raw controls unclassified until the server describes the policy", () => {
  const thread: Thread = {
    ...newThread("project", "worktree", "claude"),
    config: {},
    configurations: [
      {
        id: "permissionMode",
        type: "select" as const,
        label: "Permissions",
        application: "next_turn" as const,
        value: "acceptEdits",
        choices: [
          {
            id: "default",
            label: "Default",
          },
          {
            id: "acceptEdits",
            label: "Accept edits",
          },
          {
            id: "bypassPermissions",
            label: "Bypass",
          },
        ],
      },
    ],
  };
  const result = sessionPermissions(thread);
  expect(result.label).toBe("Permissions");
  expect(result.options.every((option) => !option.available)).toBe(true);
  expect(result.options.find((option) => option.id === "ask")?.patch).toEqual({
    executionProfile: "ask",
  });
});
it("disables common choices whose advertised application requires a new session", () => {
  const thread: Thread = {
    ...newThread("project", "worktree", "claude"),
    sessionId: "native",
    executionPolicy: {
      revision: 0,
      requested: null,
      status: "unknown",
      planned: null,
      plannedProfile: null,
      observed: null,
      controlledSettings: [],
      dispatchBlocked: false,
      reason: null,
      bindingId: null,
      nativeSession: null,
      choices: [
        {
          profile: "full",
          label: "Full access",
          description: "Server coverage",
          application: "creation_only",
          mapping: "equivalent",
          capability: {
            availability: "available",
            source: "harness",
            evidence: "declared",
          },
        },
      ],
    },
  };
  expect(sessionPermissions(thread).options.find((option) => option.id === "full")).toMatchObject({
    available: true,
    locked: true,
  });
});
it("toggles native Fast mode without changing reasoning effort", async () => {
  localStorage.clear();
  const store = new WorkspaceStore();
  await store.ready();
  store.createThread();
  const thread = {
    ...requireFixtureValue(store.active()),
    worktreeId: "/Documents",
    configurations: [
      {
        id: "model",
        type: "select" as const,
        label: "Model",
        application: "next_turn" as const,
        value: "native",
        choices: [
          {
            id: "native",
            label: "Native",
          },
        ],
      },
      {
        id: "effort",
        type: "select" as const,
        label: "Effort",
        application: "next_turn" as const,
        value: "high",
        choices: [
          {
            id: "low",
            label: "Low",
          },
          {
            id: "high",
            label: "High",
          },
        ],
      },
      {
        id: "fast",
        type: "boolean" as const,
        label: "Fast mode",
        application: "next_turn" as const,
        value: false,
      },
    ],
  };
  vi.spyOn(store, "previewControls").mockResolvedValue();
  const configure = vi.spyOn(store, "configure").mockResolvedValue();
  const view = render(<SessionControls thread={thread} store={store} connected />);
  await userEvent.click(
    screen.getByRole("button", {
      name: "Select model: Native, High",
    }),
  );
  await waitFor(() => {
    expect(
      fixtureControl(
        screen.getByRole("button", {
          name: "Fast mode",
        }),
      ).disabled,
    ).toBe(false);
  });
  await userEvent.click(
    screen.getByRole("button", {
      name: "Fast mode",
    }),
  );
  expect(configure).toHaveBeenCalledExactlyOnceWith(thread.id, "fast", true);
  expect(screen.getByRole("slider").getAttribute("aria-valuetext")).toBe("High");
  view.rerender(
    <SessionControls
      thread={{
        ...thread,
        configurations: thread.configurations.filter((field) => field.id !== "fast"),
      }}
      store={store}
      connected
    />,
  );
  expect(
    fixtureControl(
      screen.getByRole("button", {
        name: "Fast mode",
      }),
    ).disabled,
  ).toBe(true);
});
it("keeps cached controls while busy and refreshes when idle without a session-start fallback", async () => {
  localStorage.clear();
  const store = new WorkspaceStore();
  await store.ready();
  store.createThread();
  const thread = {
    ...requireFixtureValue(store.active()),
    worktreeId: "/Documents",
    state: "running",
    configurations: [
      {
        id: "model",
        type: "select" as const,
        label: "Model",
        application: "next_turn" as const,
        value: "native",
        choices: [
          {
            id: "native",
            label: "Native",
          },
        ],
      },
    ],
  };
  const preview = vi
    .spyOn(store, "previewControls")
    .mockRejectedValue(new Error("session is busy"));
  const view = render(<SessionControls thread={thread} store={store} connected />);
  await userEvent.click(
    screen.getByRole("button", {
      name: "Select model: Native",
    }),
  );
  expect(preview).not.toHaveBeenCalled();
  expect(
    fixtureControl(
      screen.getByRole("combobox", {
        name: "Choose model: Native",
      }),
    ).disabled,
  ).toBe(true);
  view.rerender(
    <SessionControls
      thread={{
        ...thread,
        state: "idle",
      }}
      store={store}
      connected
    />,
  );
  await waitFor(() => {
    expect(preview).toHaveBeenCalledOnce();
  });
  expect(screen.queryByText("session is busy")).toBeNull();
  expect(
    screen.queryByRole("button", {
      name: "Start session to load models",
    }),
  ).toBeNull();
});
it("explains unavailable configuration and does not enable it as a truthy capability object", async () => {
  localStorage.clear();
  const store = new WorkspaceStore();
  await store.ready();
  store.createThread();
  const thread: Thread = {
    ...requireFixtureValue(store.active()),
    worktreeId: "/Documents",
    sessionId: "native",
    capabilities: {
      configure: {
        availability: "unavailable",
        source: "harness",
        evidence: "declared",
        reason: "Settings require a new session",
      },
    },
    configurations: [
      {
        id: "model",
        label: "Model",
        type: "select",
        value: "native",
        choices: [
          {
            id: "native",
            label: "Native",
          },
        ],
        application: "creation_only",
      },
    ],
  };
  vi.spyOn(store, "previewControls").mockResolvedValue();
  render(<SessionControls thread={thread} store={store} connected />);
  await userEvent.click(
    screen.getByRole("button", {
      name: "Select model: Native",
    }),
  );
  await waitFor(() => {
    expect(screen.getByText("Settings require a new session")).toBeTruthy();
  });
  expect(
    fixtureControl(
      screen.getByRole("combobox", {
        name: "Choose model: Native",
      }),
    ).disabled,
  ).toBe(true);
});
function requireFixtureValue<T>(value: T | null | undefined): T {
  if (value === undefined || value === null) {
    throw new Error("Expected the fixture value to be present.");
  }
  return value;
}
