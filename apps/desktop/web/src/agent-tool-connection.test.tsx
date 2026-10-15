import { cleanup, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";

import { AgentToolConnection } from "./agent-tool-connection";
import { CodingAgentsPanel } from "./coding-agents-panel";
import { decodeObjectResult } from "./connections";
import { primaryAgentTools } from "./harness-setup";
import { setupAuthorizationUrl } from "./provider-setup";
import type { AgentToolSetup } from "./provider-setup";
import { deferred, fixtureControl, renderWithQuery as render } from "./test-fixtures";
import { WorkspaceStore } from "./workspace";

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
const descriptor = (harness = "claude"): AgentToolSetup => ({
  harness,
  installed: true,
  authState: "unknown",
  storage: {
    persistent: false,
    description: "Session only",
  },
  providers: [
    {
      id: "anthropic",
      label: "Anthropic",
      methods: [
        {
          id: "api-key",
          label: "API key",
          kind: "api_key",
          fields: [
            {
              id: "apiKey",
              label: "API key",
              secret: true,
            },
          ],
        },
      ],
    },
  ],
});
async function fixture(setup = descriptor()) {
  const store = new WorkspaceStore();
  await store.ready();
  Object.assign(store.getSnapshot(), {
    node,
    connected: true,
    harnesses: [
      {
        harness: setup.harness,
        available: true,
      },
    ],
  });
  const calls: {
    method: string;
    args: unknown;
  }[] = [];
  const rpc = vi.spyOn(store, "request").mockImplementation(async (method, args) => {
    calls.push({
      method,
      args: structuredClone(args),
    });
    if (method === "runtime/agentToolsSetup") {
      return setup;
    }
    if (method === "runtime/agentToolsConnect") {
      return {
        id: "connected",
        status: "connected",
      };
    }
    if (method === "runtime/probe") {
      return store
        .getSnapshot()
        .harnesses.find((item) => item.harness === decodeObjectResult(args).harness);
    }
    throw new Error("Unexpected fixture method");
  });
  const props = {
    store,
    harness: setup.harness,
    name: "Claude Code",
    nodeName: "This computer",
    onClose: vi.fn<(...args: unknown[]) => void>(),
    fallback: <p>Native setup guide</p>,
  };
  return {
    store,
    calls,
    rpc,
    props,
    setup,
  };
}
it("sends a password field only to the connection RPC and clears it before awaiting a result", async () => {
  const { store, calls, props } = await fixture();
  render(<AgentToolConnection {...props} />);
  const input = fixtureControl(await screen.findByLabelText("API key"));
  expect(input.type).toBe("password");
  expect(screen.getByText(/Secure storage is unavailable/u)).toBeTruthy();
  await userEvent.type(input, "fixture-private-key");
  await userEvent.click(
    screen.getByRole("button", {
      name: "Connect",
    }),
  );
  expect(input.value).toBe("");
  await screen.findByText("Connection saved. Check its status below.");
  const connect = requireFixtureValue(
    calls.find((call) => call.method === "runtime/agentToolsConnect"),
  );
  expect(connect.args).toEqual({
    harness: "claude",
    provider: "anthropic",
    method: "api-key",
    values: {
      apiKey: "fixture-private-key",
    },
    storage: "session",
  });
  expect(JSON.stringify(store.getSnapshot())).not.toContain("fixture-private-key");
  expect(JSON.stringify(localStorage)).not.toContain("fixture-private-key");
  expect(document.body.textContent).not.toContain("fixture-private-key");
  expect(calls.some((call) => call.method.includes("sessionCreate"))).toBe(false);
});
it("defaults to verified secure storage only when the runtime offers it", async () => {
  const setup = descriptor();
  setup.storage = {
    persistent: true,
    description: "Stored in this computer's credential vault.",
  };
  const { props, calls } = await fixture(setup);
  render(<AgentToolConnection {...props} />);
  await userEvent.type(await screen.findByLabelText("API key"), "fixture-secure-key");
  expect(
    screen.getByRole("combobox", {
      name: "Claude Code credential storage",
    }).textContent,
  ).toContain("Secure storage");
  await userEvent.click(
    screen.getByRole("button", {
      name: "Connect",
    }),
  );
  await waitFor(() => {
    expect(calls.some((call) => call.method === "runtime/agentToolsConnect")).toBe(true);
  });
  expect(
    decodeObjectResult(
      requireFixtureValue(calls.find((call) => call.method === "runtime/agentToolsConnect")).args,
    ).storage,
  ).toBe("secure");
});
it("can install missing browser sign-in support for an already installed agent and cancel it", async () => {
  const setup = descriptor("cursor");
  setup.installation = {
    supported: true,
    message: "Requires Node.js and npm on this computer.",
  };
  const { props, rpc, calls } = await fixture(setup);
  rpc.mockImplementation(async (method, args) => {
    calls.push({
      method,
      args: structuredClone(args),
    });
    if (method === "runtime/agentToolsSetup") {
      return setup;
    }
    if (method === "runtime/agentToolsInstall") {
      return {
        id: "support-install",
        status: "pending",
        operation: "install",
        message: "Installing sign-in support",
      };
    }
    if (method === "runtime/agentToolsCancel") {
      return {
        id: "support-install",
        status: "cancelled",
        operation: "install",
      };
    }
    throw new Error("Unexpected fixture method");
  });
  render(<AgentToolConnection {...props} name="Cursor" />);
  await userEvent.click(
    await screen.findByRole("button", {
      name: "Install sign-in support",
    }),
  );
  expect(await screen.findByText("Installing sign-in support")).toBeTruthy();
  expect(
    screen.queryByRole("button", {
      name: "Connect",
    }),
  ).toBeNull();
  await userEvent.click(
    screen.getByRole("button", {
      name: "Cancel installation",
    }),
  );
  expect(await screen.findByText("Installation cancelled.")).toBeTruthy();
  expect(
    await screen.findByRole("button", {
      name: "Install sign-in support",
    }),
  ).toBeTruthy();
  expect(calls.find((call) => call.method === "runtime/agentToolsInstall")?.args).toEqual({
    harness: "cursor",
  });
  expect(calls.some((call) => call.method === "runtime/agentToolsConnect")).toBe(false);
});
it("offers browser sign-in after installing the required support without starting authorization automatically", async () => {
  const setup = descriptor("cursor");
  setup.installation = {
    supported: true,
  };
  const { props, rpc, calls } = await fixture(setup);
  rpc.mockImplementation(async (method, args) => {
    calls.push({
      method,
      args: structuredClone(args),
    });
    if (method === "runtime/agentToolsSetup") {
      return setup;
    }
    if (method === "runtime/agentToolsInstall") {
      setup.providers[0].methods.unshift({
        id: "browser",
        label: "Sign in with Cursor",
        kind: "browser",
      });
      return {
        id: "support-installed",
        status: "completed",
        operation: "install",
      };
    }
    throw new Error("Unexpected fixture method");
  });
  render(<AgentToolConnection {...props} name="Cursor" />);
  await userEvent.click(
    await screen.findByRole("button", {
      name: "Install sign-in support",
    }),
  );
  expect(
    await screen.findByRole("button", {
      name: "Start sign-in",
    }),
  ).toBeTruthy();
  expect(
    screen.queryByRole("button", {
      name: "Install sign-in support",
    }),
  ).toBeNull();
  expect(calls.some((call) => call.method === "runtime/agentToolsConnect")).toBe(false);
});
it("does not display or retain a credential echoed in a rejected request", async () => {
  const { props, rpc, store } = await fixture();
  rpc.mockImplementation(async (method) => {
    if (method === "runtime/agentToolsSetup") {
      return descriptor();
    }
    throw new Error("Rejected fixture-private-key");
  });
  render(<AgentToolConnection {...props} />);
  const input = fixtureControl(await screen.findByLabelText("API key"));
  await userEvent.type(input, "fixture-private-key");
  await userEvent.click(
    screen.getByRole("button", {
      name: "Connect",
    }),
  );
  expect(await screen.findByRole("alert")).toHaveProperty(
    "textContent",
    "Could not connect. Check your sign-in details and try again.",
  );
  expect(input.value).toBe("");
  expect(document.body.textContent).not.toContain("fixture-private-key");
  expect(JSON.stringify(store.getSnapshot())).not.toContain("fixture-private-key");
});
it("shows runtime browser and device instructions, accepts transient input and preserves an accepted connection on close", async () => {
  const setup = descriptor("codex");
  setup.providers[0].methods = [
    {
      id: "device",
      label: "Browser sign-in",
      kind: "device",
    },
  ];
  const { props, rpc, calls } = await fixture(setup);
  rpc.mockImplementation(async (method, args) => {
    calls.push({
      method,
      args: structuredClone(args),
    });
    if (method === "runtime/agentToolsSetup") {
      return setup;
    }
    if (method === "runtime/agentToolsConnect") {
      return {
        id: "native-flow",
        status: "pending",
        authorizationUrl: "https://auth.example.com/device",
        userCode: "ABC-123",
        message: "Finish signing in in your browser.",
        inputFields: [
          {
            id: "code",
            label: "Authorization code",
            secret: true,
          },
        ],
      };
    }
    if (method === "runtime/agentToolsInput") {
      setup.connection = {
        provider: "anthropic",
        method: "device",
        storage: "native",
        verified: true,
      };
      return {
        id: "native-flow",
        status: "connected",
      };
    }
    throw new Error("Unexpected fixture method");
  });
  const view = render(<AgentToolConnection {...props} />);
  await userEvent.click(
    await screen.findByRole("button", {
      name: "Start sign-in",
    }),
  );
  expect(await screen.findByText("Finish signing in in your browser.")).toBeTruthy();
  expect(
    screen
      .getByRole("link", {
        name: "Open sign-in page",
      })
      .getAttribute("href"),
  ).toBe("https://auth.example.com/device");
  expect(screen.getByText("ABC-123")).toBeTruthy();
  const input = fixtureControl(screen.getByLabelText("Authorization code"));
  await userEvent.type(input, "private-callback-code");
  await userEvent.click(
    screen.getByRole("button", {
      name: "Continue sign-in",
    }),
  );
  expect(input.value).toBe("");
  await screen.findByText("Verified connection");
  expect(
    screen.getByText("Sign-in is managed by this agent tool's own account storage."),
  ).toBeTruthy();
  view.unmount();
  expect(calls.some((call) => call.method === "runtime/agentToolsCancel")).toBe(false);
});
it("cancels an unfinished native flow explicitly and never removes a stored connection", async () => {
  const setup = descriptor();
  setup.providers[0].methods = [
    {
      id: "browser",
      label: "Browser sign-in",
      kind: "browser",
    },
  ];
  const { props, rpc, calls } = await fixture(setup);
  rpc.mockImplementation(async (method, args) => {
    calls.push({
      method,
      args: structuredClone(args),
    });
    if (method === "runtime/agentToolsSetup") {
      return setup;
    }
    if (method === "runtime/agentToolsConnect") {
      return {
        id: "native-flow",
        status: "pending",
      };
    }
    if (method === "runtime/agentToolsCancel") {
      return {
        id: "native-flow",
        status: "cancelled",
      };
    }
    throw new Error("Unexpected fixture method");
  });
  const view = render(<AgentToolConnection {...props} />);
  await userEvent.click(
    await screen.findByRole("button", {
      name: "Start sign-in",
    }),
  );
  await userEvent.click(
    await screen.findByRole("button", {
      name: "Cancel sign-in",
    }),
  );
  await screen.findByText("Sign-in cancelled.");
  view.unmount();
  await waitFor(() => {
    expect(calls.filter((call) => call.method === "runtime/agentToolsCancel")).toHaveLength(1);
  });
  expect(calls.some((call) => call.method === "runtime/agentToolsDisconnect")).toBe(false);
});
it("ignores an earlier node's pending sign-in receipt after the execution computer changes", async () => {
  const setup = descriptor();
  setup.providers[0].methods = [
    {
      id: "browser",
      label: "Browser sign-in",
      kind: "browser",
    },
  ];
  const { props, rpc, store } = await fixture(setup);
  const response = deferred<unknown>();
  const { resolve } = response;
  rpc.mockImplementation(async (method) =>
    method === "runtime/agentToolsSetup" ? setup : await response.promise,
  );
  const view = render(<AgentToolConnection {...props} />);
  await userEvent.click(
    await screen.findByRole("button", {
      name: "Start sign-in",
    }),
  );
  Object.assign(store.getSnapshot(), {
    node: {
      ...node,
      environmentId: "other-node",
    },
  });
  view.rerender(<AgentToolConnection {...props} nodeName="Other computer" />);
  resolve({
    id: "old-flow",
    status: "pending",
    userCode: "OLD-SECRET-CODE",
    authorizationUrl: "https://auth.example.com/old",
  });
  await screen.findByRole("button", {
    name: "Start sign-in",
  });
  expect(screen.queryByText("OLD-SECRET-CODE")).toBeNull();
  expect(
    screen.queryByRole("link", {
      name: "Open sign-in page",
    }),
  ).toBeNull();
  expect(rpc.mock.calls.some(([method]) => method === "runtime/agentToolsCancel")).toBe(false);
});
it("installs from a runtime-owned flow and keeps account authentication separate", async () => {
  const setup = descriptor("antigravity");
  setup.installed = false;
  setup.installation = {
    supported: true,
    message: "Install on this execution computer.",
  };
  const { props, rpc } = await fixture(setup);
  rpc.mockImplementation(async (method) => {
    if (method === "runtime/agentToolsSetup") {
      return setup;
    }
    if (method === "runtime/agentToolsInstall") {
      setup.installed = true;
      return {
        id: "install-flow",
        operation: "install",
        status: "completed",
      };
    }
    throw new Error("Unexpected fixture method");
  });
  render(<AgentToolConnection {...props} name="Google Antigravity" />);
  await userEvent.click(
    await screen.findByRole("button", {
      name: "Install Google Antigravity",
    }),
  );
  await screen.findByLabelText("API key");
  expect(
    screen.getByText("Installation complete. Connect your account to get started."),
  ).toBeTruthy();
  expect(screen.queryByText("Verified connection")).toBeNull();
});
it("waits for a terminal installation receipt before declaring cancellation", async () => {
  const setup = descriptor("antigravity");
  setup.installed = false;
  setup.installation = {
    supported: true,
  };
  setup.flow = {
    id: "cancelling-install",
    operation: "install",
    status: "pending",
  };
  const { props, rpc } = await fixture(setup);
  rpc.mockImplementation(async (method) => {
    if (method === "runtime/agentToolsSetup") {
      return setup;
    }
    if (method === "runtime/agentToolsCancel") {
      return setup.flow;
    }
    if (method === "runtime/agentToolsFlow") {
      return {
        id: "cancelling-install",
        operation: "install",
        status: "cancelled",
      };
    }
    throw new Error("Unexpected fixture method");
  });
  render(<AgentToolConnection {...props} />);
  await userEvent.click(
    await screen.findByRole("button", {
      name: "Cancel installation",
    }),
  );
  await screen.findByText("Cancelling installation…");
  expect(screen.queryByText("Installation cancelled.")).toBeNull();
  await screen.findByText(
    "Installation cancelled.",
    {},
    {
      timeout: 2500,
    },
  );
  expect(
    screen.queryByRole("button", {
      name: "Cancel installation",
    }),
  ).toBeNull();
});
it("shows all seven primary tool names and keeps generic ACP entries in the expanded list", async () => {
  const { store } = await fixture();
  store.getSnapshot().harnesses = [
    ...primaryAgentTools.map((harness) => ({
      harness,
      available: false,
    })),
    {
      harness: "custom-acp",
      available: true,
    },
  ];
  render(<CodingAgentsPanel store={store} nodeName="This computer" />);
  for (const name of [
    "Codex",
    "Claude Code",
    "Cursor",
    "Grok Build",
    "OpenCode",
    "Google Antigravity",
    "Pi",
  ]) {
    expect(screen.getByText(name)).toBeTruthy();
  }
  expect(screen.queryByText("custom-acp")).toBeNull();
  await userEvent.click(
    screen.getByRole("button", {
      name: "Show 1 more agent tools",
    }),
  );
  expect(await screen.findByText("custom-acp")).toBeTruthy();
});
it("rejects non-web and credential-bearing authorization links", () => {
  for (const scheme of ["javascript", "file", "data"]) {
    expect(setupAuthorizationUrl(`${scheme}:alert(1)`)).toBeUndefined();
  }
  expect(setupAuthorizationUrl("https://user:secret@example.com/login")).toBeUndefined();
  expect(setupAuthorizationUrl("https://example.com/login")).toBe("https://example.com/login");
});
it("selects the model provider with keyboard controls and clears a previous provider's key", async () => {
  const setup = descriptor("pi");
  setup.providers.unshift({
    id: "openai",
    label: "OpenAI",
    methods: [
      {
        id: "api-key",
        label: "API key",
        kind: "api_key",
        fields: [
          {
            id: "apiKey",
            label: "API key",
            secret: true,
          },
        ],
      },
    ],
  });
  const { props, calls } = await fixture(setup);
  render(<AgentToolConnection {...props} name="Pi" />);
  const input = fixtureControl(await screen.findByLabelText("API key"));
  await userEvent.type(input, "old-provider-key");
  await userEvent.click(
    screen.getByRole("combobox", {
      name: "Pi model provider",
    }),
  );
  await userEvent.keyboard("{ArrowDown}{Enter}");
  expect(
    screen.getByRole("combobox", {
      name: "Pi model provider",
    }).textContent,
  ).toContain("Anthropic");
  expect(input.value).toBe("");
  await userEvent.type(input, "new-provider-key");
  await userEvent.click(
    screen.getByRole("button", {
      name: "Connect",
    }),
  );
  await waitFor(() => {
    expect(calls.some((call) => call.method === "runtime/agentToolsConnect")).toBe(true);
  });
  expect(
    decodeObjectResult(
      requireFixtureValue(calls.find((call) => call.method === "runtime/agentToolsConnect")).args,
    ).provider,
  ).toBe("anthropic");
});
it("cancels a same-node pending login on unmount and preserves a generic ACP tool's detection", async () => {
  const setup = descriptor("custom-acp");
  setup.manual = true;
  setup.installed = false;
  setup.providers = [];
  const { props, store } = await fixture(setup);
  render(<AgentToolConnection {...props} name="Custom ACP" />);
  await screen.findByText("Native setup guide");
  expect(store.getSnapshot().harnesses[0].available).toBe(true);
  expect(screen.getByText("Connect Custom ACP")).toBeTruthy();
  cleanup();
  const native = descriptor();
  native.providers[0].methods = [
    {
      id: "browser",
      label: "Browser",
      kind: "browser",
    },
  ];
  const fixtureState = await fixture(native);
  fixtureState.rpc.mockImplementation(async (method, args) => {
    fixtureState.calls.push({
      method,
      args: structuredClone(args),
    });
    if (method === "runtime/agentToolsSetup") {
      return native;
    }
    if (method === "runtime/agentToolsConnect") {
      return {
        id: "pending-owned-flow",
        status: "pending",
      };
    }
    if (method === "runtime/agentToolsCancel") {
      return {
        id: "pending-owned-flow",
        status: "cancelled",
      };
    }
    throw new Error("Unexpected fixture method");
  });
  const view = render(<AgentToolConnection {...fixtureState.props} />);
  await userEvent.click(
    await screen.findByRole("button", {
      name: "Start sign-in",
    }),
  );
  await screen.findByRole("button", {
    name: "Cancel sign-in",
  });
  view.unmount();
  await waitFor(() => {
    expect(
      fixtureState.calls.filter((call) => call.method === "runtime/agentToolsCancel"),
    ).toHaveLength(1);
  });
});
it("polls native progress without repeating the login start", async () => {
  const setup = descriptor();
  setup.providers[0].methods = [
    {
      id: "browser",
      label: "Browser sign-in",
      kind: "browser",
    },
  ];
  const { props, rpc, calls } = await fixture(setup);
  rpc.mockImplementation(async (method, args) => {
    calls.push({
      method,
      args: structuredClone(args),
    });
    if (method === "runtime/agentToolsSetup") {
      return setup;
    }
    if (method === "runtime/agentToolsConnect") {
      return {
        id: "polled-flow",
        status: "pending",
      };
    }
    if (method === "runtime/agentToolsFlow") {
      setup.connection = {
        provider: "anthropic",
        method: "browser",
        storage: "native",
        verified: true,
      };
      return {
        id: "polled-flow",
        status: "connected",
      };
    }
    throw new Error("Unexpected fixture method");
  });
  const view = render(<AgentToolConnection {...props} />);
  await userEvent.click(
    await screen.findByRole("button", {
      name: "Start sign-in",
    }),
  );
  await screen.findByText(
    "Verified connection",
    {},
    {
      timeout: 2500,
    },
  );
  view.unmount();
  expect(calls.filter((call) => call.method === "runtime/agentToolsConnect")).toHaveLength(1);
  expect(calls.filter((call) => call.method === "runtime/agentToolsFlow")).toHaveLength(1);
  expect(calls.some((call) => call.method === "runtime/agentToolsCancel")).toBe(false);
});
it.each(["connect", "install"] as const)(
  "refreshes an open form after a polled %s receipt even when setup loads later",
  async (operation) => {
    const setup = descriptor(operation === "install" ? "antigravity" : "claude");
    setup.installed = operation !== "install";
    setup.installation = {
      supported: operation === "install",
    };
    setup.providers[0].methods = [
      {
        id: "browser",
        label: "Browser",
        kind: "browser",
      },
    ];
    setup.flow = {
      id: "delayed-terminal",
      status: "pending",
      operation,
    };
    const { props, rpc } = await fixture(setup);
    let reads = 0;
    const response = deferred<AgentToolSetup>();
    const finishRefresh = response.resolve;
    rpc.mockImplementation(async (method) => {
      if (method === "runtime/agentToolsSetup") {
        reads += 1;
        if (reads === 1) {
          return structuredClone(setup);
        }
        return await response.promise;
      }
      if (method === "runtime/agentToolsFlow") {
        return {
          id: "delayed-terminal",
          status: operation === "install" ? "completed" : "connected",
          operation,
        };
      }
      throw new Error("Unexpected fixture method");
    });
    render(<AgentToolConnection {...props} />);
    await screen.findByText(
      operation === "install"
        ? "Installation complete. Connect your account to get started."
        : "Connection saved. Check its status below.",
      {},
      {
        timeout: 2500,
      },
    );
    finishRefresh({
      ...setup,
      installed: true,
      flow: undefined,
      ...(operation === "connect"
        ? {
            connection: {
              provider: "anthropic",
              method: "browser",
              storage: "native",
              verified: true,
            },
          }
        : {}),
    });
    await (operation === "connect"
      ? screen.findByText("Verified connection")
      : screen.findByRole("button", { name: "Start sign-in" }));
  },
);
it.each(["connect", "install"] as const)(
  "cancels a late initial pending %s receipt when its form closes on the same node",
  async (operation) => {
    const setup = descriptor(operation === "install" ? "antigravity" : "claude");
    setup.installed = operation !== "install";
    setup.installation = {
      supported: operation === "install",
    };
    setup.providers[0].methods = [
      {
        id: "browser",
        label: "Browser",
        kind: "browser",
      },
    ];
    const { props, rpc, calls } = await fixture(setup);
    const response = deferred<unknown>();
    const finishStart = response.resolve;
    rpc.mockImplementation(async (method, args) => {
      calls.push({
        method,
        args: structuredClone(args),
      });
      if (method === "runtime/agentToolsSetup") {
        return setup;
      }
      if (method === "runtime/agentToolsConnect" || method === "runtime/agentToolsInstall") {
        return await response.promise;
      }
      if (method === "runtime/agentToolsCancel") {
        return {
          id: "late-pending",
          status: "cancelled",
        };
      }
      throw new Error("Unexpected fixture method");
    });
    const view = render(<AgentToolConnection {...props} />);
    await userEvent.click(
      await screen.findByRole("button", {
        name: operation === "install" ? "Install Claude Code" : "Start sign-in",
      }),
    );
    view.unmount();
    finishStart({
      id: "late-pending",
      status: "pending",
      operation,
    });
    await waitFor(() => {
      expect(calls.filter((call) => call.method === "runtime/agentToolsCancel")).toEqual([
        {
          method: "runtime/agentToolsCancel",
          args: {
            id: "late-pending",
          },
        },
      ]);
    });
  },
);
it("shows explicit session-only storage for Cursor's browser-minted API key", async () => {
  const setup = descriptor("cursor");
  setup.providers[0].methods = [
    {
      id: "browser",
      label: "Sign in with Cursor",
      kind: "browser",
    },
  ];
  const { props, calls } = await fixture(setup);
  render(<AgentToolConnection {...props} name="Cursor" />);
  const storage = await screen.findByRole("combobox", {
    name: "Cursor credential storage",
  });
  expect(storage.textContent).toContain("This runtime session only");
  expect(screen.getByText(/Secure storage is unavailable/u)).toBeTruthy();
  await userEvent.click(
    screen.getByRole("button", {
      name: "Start sign-in",
    }),
  );
  await waitFor(() => {
    expect(calls.some((call) => call.method === "runtime/agentToolsConnect")).toBe(true);
  });
  expect(
    decodeObjectResult(
      requireFixtureValue(calls.find((call) => call.method === "runtime/agentToolsConnect")).args,
    ).storage,
  ).toBe("session");
});
it("allows removing a Swarmie-owned credential when the tool is no longer installed", async () => {
  const setup = descriptor();
  setup.installed = false;
  setup.connection = {
    provider: "anthropic",
    method: "api-key",
    storage: "secure",
    verified: false,
  };
  setup.message = "Install the missing agent tool to verify this connection.";
  const { props, rpc } = await fixture(setup);
  rpc.mockImplementation(async (method) => {
    if (method === "runtime/agentToolsSetup") {
      return setup;
    }
    if (method === "runtime/agentToolsDisconnect") {
      delete setup.connection;
      return setup;
    }
    throw new Error("Unexpected fixture method");
  });
  render(<AgentToolConnection {...props} />);
  expect(await screen.findByText("Connection configured")).toBeTruthy();
  expect(
    screen.getByText("Install the missing agent tool to verify this connection."),
  ).toBeTruthy();
  await userEvent.click(
    screen.getByRole("button", {
      name: "Disconnect in Swarmie",
    }),
  );
  await screen.findByText(
    "Connection removed from Swarmie. Your agent tool's own login is unchanged.",
  );
  expect(screen.queryByText("Connection configured")).toBeNull();
});
it("keeps an already-started native login cancellable if the tool becomes unavailable", async () => {
  const setup = descriptor();
  setup.installed = false;
  setup.providers[0].methods = [
    {
      id: "browser",
      label: "Browser",
      kind: "browser",
    },
  ];
  setup.flow = {
    id: "existing-pending-flow",
    status: "pending",
  };
  const { props, rpc } = await fixture(setup);
  rpc.mockImplementation(async (method) => {
    if (method === "runtime/agentToolsSetup") {
      return setup;
    }
    if (method === "runtime/agentToolsCancel") {
      return {
        id: "existing-pending-flow",
        status: "cancelled",
      };
    }
    throw new Error("Unexpected fixture method");
  });
  render(<AgentToolConnection {...props} />);
  await userEvent.click(
    await screen.findByRole("button", {
      name: "Cancel sign-in",
    }),
  );
  await screen.findByText("Sign-in cancelled.");
});
function requireFixtureValue<T>(value: T | null | undefined): T {
  if (value === undefined || value === null) {
    throw new Error("Expected the fixture value to be present.");
  }
  return value;
}
