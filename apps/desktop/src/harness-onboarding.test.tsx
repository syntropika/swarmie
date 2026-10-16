import { cleanup, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";

import { decodeObjectResult } from "~/src/connections";
import { HarnessOnboarding } from "~/src/harness-onboarding";
import { HarnessSelect } from "~/src/harness-select";
import {
  completeHarnessSetup,
  defaultHarness,
  harnessReadiness,
  readHarnessSetup,
} from "~/src/harness-setup";
import { NodeClient } from "~/src/nodes";
import type { Detection } from "~/src/protocol";
import { SettingsPage } from "~/src/settings-page";
import { defaultShortcuts } from "~/src/shortcuts";
import { fixtureControl, renderWithQuery as render } from "~/src/test-fixtures";
import { WorkspaceStore } from "~/src/workspace";

const node = {
  environmentId: "node",
  storeId: "store",
  ownerScopeId: "owner",
};
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  localStorage.clear();
});
async function setup(harnesses: Detection[], connected = true) {
  localStorage.clear();
  const store = new WorkspaceStore();
  await store.ready();
  Object.assign(store.getSnapshot(), {
    node,
    connected,
    harnesses,
  });
  const rpc = vi.spyOn(store, "request").mockImplementation(async (_method, args: unknown) => ({
    ...harnesses.find((item) => item.harness === decodeObjectResult(args).harness),
    authState: "unknown",
  }));
  vi.spyOn(store, "agentToolsSetup").mockImplementation(async (harness) => ({
    harness,
    installed:
      store.getSnapshot().harnesses.find((item) => item.harness === harness)?.available === true,
    authState: "unknown",
    storage: {
      persistent: false,
      description: "Session only",
    },
    providers: [],
  }));
  const finish = vi.fn<(...args: unknown[]) => void>();
  return {
    store,
    rpc,
    finish,
  };
}
it("never equates an installed binary or an unknown login with authenticated readiness", async () => {
  const { store, rpc, finish } = await setup([
    {
      harness: "codex",
      available: true,
    },
    {
      harness: "pi",
      available: false,
    },
  ]);
  render(<HarnessOnboarding store={store} nodeName="This computer" onFinish={finish} />);
  await waitFor(() => {
    expect(screen.getByText("Authentication has not been confirmed.")).toBeTruthy();
  });
  expect(screen.queryByText("Ready")).toBeNull();
  expect(rpc).toHaveBeenCalledTimes(1);
  expect(rpc).toHaveBeenCalledWith("runtime/probe", {
    harness: "codex",
  });
  expect(
    fixtureControl(
      screen.getByRole("switch", {
        name: "Use Pi",
      }),
    ).disabled,
  ).toBe(true);
  await userEvent.click(
    screen.getByRole("button", {
      name: "Finish setup",
    }),
  );
  expect(finish).toHaveBeenCalledWith();
});
it("reuses an authenticated login without presenting a second sign-in", async () => {
  const { store, rpc, finish } = await setup([
    {
      harness: "codex",
      available: true,
    },
  ]);
  rpc.mockResolvedValue({
    harness: "codex",
    available: true,
    authState: "authenticated",
  });
  render(<HarnessOnboarding store={store} nodeName="This computer" onFinish={finish} />);
  expect(await screen.findByText("Ready")).toBeTruthy();
  expect(
    screen.queryByRole("button", {
      name: "Sign in",
    }),
  ).toBeNull();
  expect(screen.getByText("Installed · Existing login detected")).toBeTruthy();
});
it("shows Cursor SDK credential guidance and reports missing SDK dependencies without claiming readiness", async () => {
  const { store, rpc, finish } = await setup([
    {
      harness: "cursor",
      available: true,
    },
  ]);
  rpc.mockResolvedValue({
    harness: "cursor",
    available: true,
    authState: "unauthenticated",
    message: "Configure CURSOR_API_KEY on this computer.",
  });
  render(<HarnessOnboarding store={store} nodeName="Execution computer" onFinish={finish} />);
  await userEvent.click(
    await screen.findByRole("button", {
      name: "Connect",
    }),
  );
  expect(screen.getByText("Configure CURSOR_API_KEY on this computer.")).toBeTruthy();
  const guide = await screen.findByRole("link", { name: "Setup guide" });
  expect(guide.getAttribute("href")).toBe("https://cursor.com/docs/sdk/bridge");
  expect(screen.queryByText("cursor-agent login")).toBeNull();
  rpc.mockResolvedValue({
    harness: "cursor",
    available: false,
    authState: "unknown",
    message: "Install the Cursor SDK Bridge on this computer.",
  });
  await userEvent.click(
    screen.getAllByRole("button", {
      name: "Check again",
    })[0],
  );
  expect(await screen.findByText("Install the Cursor SDK Bridge on this computer.")).toBeTruthy();
  expect(screen.queryByText("Ready")).toBeNull();
  expect(
    fixtureControl(
      screen.getByRole("switch", {
        name: "Use Cursor",
      }),
    ).disabled,
  ).toBe(true);
});
it("guides native sign-in and observes its result without launching a conversation", async () => {
  const { store, rpc, finish } = await setup([
    {
      harness: "claude",
      available: true,
    },
  ]);
  rpc.mockResolvedValue({
    harness: "claude",
    available: true,
    authState: "unauthenticated",
  });
  render(<HarnessOnboarding store={store} nodeName="Remote node" onFinish={finish} />);
  await userEvent.click(
    await screen.findByRole("button", {
      name: "Connect",
    }),
  );
  expect(
    await screen.findByText("claude", {
      selector: "code",
    }),
  ).toBeTruthy();
  expect(
    screen
      .getByRole("link", {
        name: "Setup guide",
      })
      .getAttribute("href"),
  ).toContain("code.claude.com");
  rpc.mockResolvedValue({
    harness: "claude",
    available: true,
    authState: "authenticated",
  });
  await userEvent.click(
    screen.getAllByRole("button", {
      name: "Check again",
    })[0],
  );
  expect(await screen.findByText("Ready")).toBeTruthy();
  expect(rpc.mock.calls.every(([method]) => method === "runtime/probe")).toBe(true);
});
it("redetects a newly installed agent and makes it available for activation", async () => {
  const { store, rpc, finish } = await setup([
    {
      harness: "pi",
      available: false,
    },
  ]);
  render(<HarnessOnboarding store={store} nodeName="This computer" onFinish={finish} />);
  await userEvent.click(
    screen.getByRole("button", {
      name: "Install",
    }),
  );
  expect(
    await screen.findByRole("link", {
      name: "Installation guide",
    }),
  ).toBeTruthy();
  rpc.mockResolvedValue({
    harness: "pi",
    available: true,
    authState: "unknown",
  });
  await userEvent.click(
    screen.getAllByRole("button", {
      name: "Check again",
    })[0],
  );
  const toggle = fixtureControl(
    screen.getByRole("switch", {
      name: "Use Pi",
    }),
  );
  await waitFor(() => {
    expect(toggle.disabled).toBe(false);
  });
  expect(toggle.matches(":checked")).toBe(true);
  await userEvent.click(toggle);
  expect(store.getSnapshot().disabledHarnesses).toContain("pi");
  await userEvent.click(toggle);
  expect(store.getSnapshot().disabledHarnesses).not.toContain("pi");
  await userEvent.click(
    screen.getByRole("button", {
      name: "Finish setup",
    }),
  );
  expect(finish).toHaveBeenCalledWith();
});
it("keeps probe errors actionable and restores readiness on an explicit recheck", async () => {
  const { store, rpc, finish } = await setup([
    {
      harness: "codex",
      available: true,
      authState: "authenticated",
    },
  ]);
  rpc.mockRejectedValue(new Error("Connection lost. Reconnect and check again."));
  render(<HarnessOnboarding store={store} nodeName="This computer" onFinish={finish} />);
  expect(await screen.findByRole("alert")).toHaveProperty(
    "textContent",
    "Connection lost. Reconnect and check again.",
  );
  expect(screen.queryByText("Ready")).toBeNull();
  rpc.mockResolvedValue({
    harness: "codex",
    available: true,
    authState: "authenticated",
  });
  await userEvent.click(
    screen.getByRole("button", {
      name: "Check again",
    }),
  );
  expect(await screen.findByText("Ready")).toBeTruthy();
  expect(screen.queryByRole("alert")).toBeNull();
});
it("allows skipping disconnected setup without probing or changing a default", async () => {
  const { store, rpc, finish } = await setup(
    [
      {
        harness: "codex",
        available: true,
      },
    ],
    false,
  );
  render(<HarnessOnboarding store={store} nodeName="Remote node" onFinish={finish} />);
  await userEvent.click(
    screen.getByRole("button", {
      name: "Skip setup",
    }),
  );
  expect(finish).toHaveBeenCalledWith();
  expect(rpc).not.toHaveBeenCalled();
});
it("scopes completion and preferences to the full retained node identity", async () => {
  localStorage.clear();
  completeHarnessSetup(node, "claude");
  expect(readHarnessSetup(node)).toEqual({
    completed: true,
    preferredHarness: "claude",
    disabledHarnesses: [],
  });
  expect(
    readHarnessSetup({
      ...node,
      storeId: "replacement",
    }).completed,
  ).toBe(false);
  completeHarnessSetup(node);
  const store = new WorkspaceStore();
  await store.ready();
  Object.assign(store.getSnapshot(), {
    node,
    harnesses: [
      {
        harness: "codex",
        available: true,
      },
      {
        harness: "claude",
        available: true,
      },
    ],
  });
  store.createThread();
  expect(store.active()?.harness).toBe("claude");
  expect(
    defaultHarness(
      [
        {
          harness: "codex",
          available: true,
        },
        {
          harness: "claude",
          available: false,
        },
      ],
      node,
    ),
  ).toBe("codex");
  expect(
    harnessReadiness({
      harness: "claude",
      available: false,
      authState: "authenticated",
    }).ready,
  ).toBe(false);
});
it("activates agents independently and shares their state with the Settings section", async () => {
  const { store, finish } = await setup([
    {
      harness: "codex",
      available: true,
    },
    {
      harness: "claude",
      available: true,
    },
  ]);
  const view = render(
    <HarnessOnboarding store={store} nodeName="This computer" onFinish={finish} />,
  );
  await waitFor(() => {
    expect(screen.getAllByText("Authentication has not been confirmed.")).toHaveLength(2);
  });
  expect(
    screen
      .getByRole("switch", {
        name: "Use Codex",
      })
      .matches(":checked"),
  ).toBe(true);
  expect(
    screen
      .getByRole("switch", {
        name: "Use Claude Code",
      })
      .matches(":checked"),
  ).toBe(true);
  await userEvent.click(
    screen.getByRole("switch", {
      name: "Use Codex",
    }),
  );
  expect(
    screen
      .getByRole("switch", {
        name: "Use Claude Code",
      })
      .matches(":checked"),
  ).toBe(true);
  expect(defaultHarness(store.getSnapshot().harnesses, node)).toBe("claude");
  view.unmount();
  const client = new NodeClient(store);
  render(
    <SettingsPage
      store={store}
      state={store.getSnapshot()}
      client={client}
      nodes={client.getSnapshot()}
      shortcuts={defaultShortcuts}
      setShortcuts={vi.fn<(...args: unknown[]) => void>()}
      onClose={vi.fn<(...args: unknown[]) => void>()}
      onSetup={vi.fn<(...args: unknown[]) => void>()}
    />,
  );
  await userEvent.click(
    screen.getByRole("button", {
      name: "Agent tools",
    }),
  );
  expect(
    screen
      .getByRole("switch", {
        name: "Use Codex",
      })
      .matches(":checked"),
  ).toBe(false);
  expect(
    screen
      .getByRole("switch", {
        name: "Use Claude Code",
      })
      .matches(":checked"),
  ).toBe(true);
  await userEvent.click(
    screen.getByRole("switch", {
      name: "Use Codex",
    }),
  );
  expect(readHarnessSetup(node).disabledHarnesses).toEqual([]);
});
it("persists an all-disabled choice and prevents new sessions without blocking an existing session", async () => {
  const { store, rpc } = await setup([
    {
      harness: "codex",
      available: true,
    },
    {
      harness: "claude",
      available: true,
    },
  ]);
  store.setHarnessEnabled("codex", false);
  store.setHarnessEnabled("claude", false);
  completeHarnessSetup(node);
  expect(defaultHarness(store.getSnapshot().harnesses, node)).toBeUndefined();
  const restored = new WorkspaceStore({
    node,
  });
  await restored.ready();
  expect(restored.getSnapshot().disabledHarnesses).toEqual(["codex", "claude"]);
  store.getSnapshot().chatDirectory = "/fixture";
  store.createThread();
  const thread = requireFixtureValue(store.active());
  store.updateThread(thread.id, {
    worktreeId: "/fixture",
  });
  await expect(store.ensureSession(thread.id)).rejects.toThrow("This agent tool is disabled");
  expect(rpc).not.toHaveBeenCalled();
  store.updateThread(thread.id, {
    sessionId: "existing",
    state: "idle",
  });
  const existingSession = await store.ensureSession(thread.id);
  expect(existingSession.sessionId).toBe("existing");
  expect(rpc).not.toHaveBeenCalled();
  expect(
    readHarnessSetup({
      ...node,
      ownerScopeId: "different",
    }).disabledHarnesses,
  ).toEqual([]);
});
it("excludes disabled agents from new chat selection and preserves an existing session's label", async () => {
  const harnesses = [
    {
      harness: "codex",
      available: true,
    },
    {
      harness: "claude",
      available: true,
    },
  ];
  const view = render(
    <HarnessSelect
      label="Agent tool"
      value="claude"
      onChange={vi.fn<(...args: unknown[]) => void>()}
      harnesses={harnesses}
      disabledHarnesses={["claude"]}
    />,
  );
  await userEvent.click(
    screen.getByRole("button", {
      name: /Agent tool/u,
    }),
  );
  expect(
    screen.queryByRole("option", {
      name: "Claude Code",
    }),
  ).toBeNull();
  expect(
    screen.getByRole("option", {
      name: "Codex",
    }),
  ).toBeTruthy();
  await userEvent.keyboard("{Escape}");
  view.rerender(
    <HarnessSelect
      label="Agent tool"
      value="claude"
      onChange={vi.fn<(...args: unknown[]) => void>()}
      harnesses={harnesses}
      disabledHarnesses={["claude"]}
      disabled
      appearance="menu"
      status={{
        state: "running",
        connected: true,
      }}
    />,
  );
  expect(
    screen.getByRole("button", {
      name: "Agent tool: Claude Code",
    }),
  ).toBeTruthy();
});
it("retains legacy setup completion and native preference when adding enablement choices", async () => {
  localStorage.clear();
  completeHarnessSetup(node, "claude");
  const store = new WorkspaceStore({
    node,
  });
  await store.ready();
  store.getSnapshot().harnesses = [
    {
      harness: "codex",
      available: true,
    },
    {
      harness: "claude",
      available: true,
    },
  ];
  store.setHarnessEnabled("claude", false);
  expect(readHarnessSetup(node)).toEqual({
    completed: true,
    preferredHarness: "claude",
    disabledHarnesses: ["claude"],
  });
  expect(defaultHarness(store.getSnapshot().harnesses, node)).toBe("codex");
  store.setHarnessEnabled("claude", true);
  expect(defaultHarness(store.getSnapshot().harnesses, node)).toBe("claude");
});
function requireFixtureValue<T>(value: T | null | undefined): T {
  if (value === undefined || value === null) {
    throw new Error("Expected the fixture value to be present.");
  }
  return value;
}
