import { cleanup, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";

import { NodeSettings } from "~/web/src/node-settings";
import { NodeClient } from "~/web/src/nodes";
import type { SshReadiness, NodesState } from "~/web/src/nodes";
import {
  fixtureControl,
  renderWithQuery as render,
  fixtureWorkspace,
  deferred,
} from "~/web/src/test-fixtures";
import { WorkspaceStore } from "~/web/src/workspace";

afterEach(cleanup);
function setup(fail = false) {
  const pair = vi.fn<NodeClient["pair"]>();
  if (fail) {
    pair.mockRejectedValue(new Error("Node identity could not be verified"));
  } else {
    pair.mockResolvedValue(undefined);
  }
  const snapshot = fixtureWorkspace({
    connected: true,
    harnesses: [],
    error: "",
    node: {
      environmentId: "local",
      storeId: "store",
      ownerScopeId: "owner",
    },
  });
  const store = new WorkspaceStore();
  Object.assign(store.getSnapshot(), snapshot);
  const nodes: NodesState = {
    nodes: [
      {
        identity: {
          environmentId: "local",
          storeId: "store",
          ownerScopeId: "owner",
        },
        displayName: "This computer",
        local: true,
        registered: true,
        connections: [],
        lastVerifiedAtMs: 0,
      },
      {
        identity: {
          environmentId: "remote",
          storeId: "remote-store",
          ownerScopeId: "owner",
        },
        displayName: "Remote",
        local: false,
        registered: true,
        lastVerifiedAtMs: 0,
        connections: [
          {
            name: "main",
            revision: 7,
            authenticated: false,
            transport: {
              kind: "http",
              url: "http://remote.invalid",
            },
          },
        ],
      },
    ],
    error: "",
    loading: false,
    store,
  };
  const inspectSsh = vi.fn<NodeClient["inspectSsh"]>().mockResolvedValue(
    readiness({
      status: "ready",
      nextAction: "connect",
      installed: true,
      ready: true,
      version: "swarmie 0.1.0",
      runtimeIdentity: {
        identity: { environmentId: "remote", storeId: "store", ownerScopeId: "owner" },
        incarnationId: "live",
      },
    }),
  );
  const prepareSsh = vi.fn<NodeClient["prepareSsh"]>();
  const pairSsh = vi.fn<NodeClient["pairSsh"]>().mockResolvedValue(undefined);
  const removeRoute = vi.fn<NodeClient["removeRoute"]>().mockResolvedValue(undefined);
  const refresh = vi.fn<NodeClient["refresh"]>().mockResolvedValue(undefined);
  const probe = vi
    .fn<NodeClient["probe"]>()
    .mockResolvedValue({ harness: "codex", available: true });
  const select = vi.fn<NodeClient["select"]>().mockResolvedValue(undefined);
  const connect = vi.fn<NodeClient["connect"]>().mockResolvedValue(undefined);
  const disconnect = vi.fn<NodeClient["disconnect"]>();
  const reconnect = vi.fn<NodeClient["reconnect"]>().mockResolvedValue(undefined);
  const client = Object.assign(new NodeClient(store), {
    pair,
    inspectSsh,
    prepareSsh,
    pairSsh,
    removeRoute,
    refresh,
    probe,
    select,
    connect,
    disconnect,
    reconnect,
    localSnapshot: () => snapshot,
    nodeSnapshot: () => snapshot,
    status: () => "Connected",
  });
  render(<NodeSettings client={client} nodes={nodes} />);
  return {
    pair,
    inspectSsh,
    prepareSsh,
    pairSsh,
    removeRoute,
    refresh,
    probe,
    select,
    connect,
    disconnect,
    reconnect,
    snapshot,
  };
}
async function openAdd(user: ReturnType<typeof userEvent.setup>) {
  await user.click(
    screen.getByRole("tab", {
      name: "Control other devices",
    }),
  );
  await user.click(
    screen.getByRole("button", {
      name: "Add",
    }),
  );
}
it("keeps all connections inside one accessible view selector with keyboard navigation", async () => {
  setup();
  const user = userEvent.setup();
  expect(screen.getByRole("tabpanel").textContent).toContain("This computer");
  expect(screen.queryByRole("switch")).toBeNull();
  await user.click(
    screen.getByRole("tab", {
      name: "Control this computer",
    }),
  );
  await user.keyboard("{ArrowRight}");
  expect(document.activeElement).toBe(
    screen.getByRole("tab", {
      name: "Control other devices",
    }),
  );
  expect(screen.getByRole("tabpanel").textContent).toContain("Remote");
  expect(
    screen
      .getByRole("tab", {
        name: "Control other devices",
      })
      .getAttribute("aria-selected"),
  ).toBe("true");
  await user.keyboard("{Home}");
  expect(screen.queryByRole("switch")).toBeNull();
});
it("submits from the keyboard and preserves inputs on failed verification, then clears canceled credentials", async () => {
  const { pair } = setup(true);
  const user = userEvent.setup();
  await openAdd(user);
  await user.type(screen.getByLabelText("Connection name"), "Remote route");
  await user.type(screen.getByLabelText("Address"), "http://localhost:7490");
  await user.type(screen.getByLabelText("Access token"), "secret");
  await user.keyboard("{Enter}");
  await waitFor(() => {
    expect(screen.getByRole("alert").textContent).toContain("could not be verified");
  });
  expect(pair).toHaveBeenCalledWith("Remote route", "http://localhost:7490", "secret");
  expect(fixtureControl(screen.getByLabelText("Address")).value).toBe("http://localhost:7490");
  await user.click(
    screen.getByRole("button", {
      name: "Cancel",
    }),
  );
  expect(document.activeElement).toBe(
    screen.getByRole("button", {
      name: "Add",
    }),
  );
  await user.click(
    screen.getByRole("button", {
      name: "Add",
    }),
  );
  expect(fixtureControl(screen.getByLabelText("Access token")).value).toBe("");
});
it("closes a successful add dialog and prevents dismissal or repeat submission during verification", async () => {
  const { pair } = setup();
  const user = userEvent.setup();
  const finishGate = deferred<undefined>();
  const finish = finishGate.resolve;
  pair.mockImplementation(async () => {
    await finishGate.request();
  });
  await openAdd(user);
  await user.type(screen.getByLabelText("Connection name"), "Route");
  await user.type(screen.getByLabelText("Address"), "http://localhost:7490");
  await user.keyboard("{Enter}{Escape}{Enter}");
  expect(screen.getByRole("dialog")).toBeTruthy();
  expect(pair).toHaveBeenCalledOnce();
  expect(
    fixtureControl(
      screen.getByRole("button", {
        name: "Cancel",
      }),
    ).disabled,
  ).toBe(true);
  finish(undefined);
  await waitFor(() => {
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
it("removes only the route revision in connection details", async () => {
  const { removeRoute } = setup();
  const user = userEvent.setup();
  await user.click(
    screen.getByRole("tab", {
      name: "Control other devices",
    }),
  );
  await user.click(
    screen.getByRole("button", {
      name: "Remote actions",
    }),
  );
  await user.click(
    screen.getByRole("menuitem", {
      name: "Details",
    }),
  );
  await user.click(
    screen.getByRole("button", {
      name: "Remove connection main",
    }),
  );
  expect(removeRoute).toHaveBeenCalledWith("main", 7);
});
it("lets a remote connection stop observation without exposing a local disconnect control", async () => {
  const { disconnect } = setup();
  const user = userEvent.setup();
  expect(screen.queryByRole("switch")).toBeNull();
  await user.click(
    screen.getByRole("button", {
      name: "This computer actions",
    }),
  );
  expect(within(screen.getByRole("menu")).queryByText(/connect/iu)).toBeNull();
  await user.keyboard("{Escape}");
  await user.click(
    screen.getByRole("tab", {
      name: "Control other devices",
    }),
  );
  await user.click(
    screen.getByRole("switch", {
      name: "Connect Remote",
    }),
  );
  expect(disconnect).toHaveBeenCalledWith("remote");
});
it("shows authentication recovery for the automatic local connection", async () => {
  const { snapshot, reconnect } = setup();
  const user = userEvent.setup();
  await user.click(
    screen.getByRole("button", {
      name: "This computer actions",
    }),
  );
  await user.click(
    screen.getByRole("menuitem", {
      name: "Details",
    }),
  );
  await user.click(
    screen.getByRole("button", {
      name: "Connection access",
    }),
  );
  snapshot.connected = false;
  snapshot.error = "Token rejected";
  await user.type(screen.getByLabelText("Local access token"), "secret");
  await user.keyboard("{Enter}");
  expect(reconnect).toHaveBeenCalledWith("secret");
  await waitFor(() => {
    expect(screen.getByRole("alert").textContent).toContain("Token rejected");
  });
  expect(screen.getByRole("dialog")).toBeTruthy();
});
it("checks SSH readiness before pairing and clears a prepared target when connection inputs change", async () => {
  const { inspectSsh, pairSsh } = setup();
  const user = userEvent.setup();
  await openAdd(user);
  await user.type(screen.getByLabelText("Connection name"), "Remote SSH");
  await user.selectOptions(screen.getByLabelText("Connection method"), "ssh");
  await user.type(screen.getByLabelText("SSH host"), "remote-host");
  await user.click(screen.getByText("Advanced"));
  await user.type(screen.getByLabelText("Runtime socket path"), "/private/runtime/node.sock");
  expect(
    fixtureControl(
      screen.getByRole("button", {
        name: "Add node",
      }),
    ).disabled,
  ).toBe(true);
  await user.click(
    screen.getByRole("button", {
      name: "Check connection",
    }),
  );
  await waitFor(() => {
    expect(screen.getByRole("status").textContent).toContain("Runtime ready");
  });
  expect(inspectSsh).toHaveBeenCalledWith("remote-host", "/private/runtime/node.sock");
  await user.type(screen.getByLabelText("SSH host"), "-changed");
  expect(
    fixtureControl(
      screen.getByRole("button", {
        name: "Add node",
      }),
    ).disabled,
  ).toBe(true);
  await user.click(
    screen.getByRole("button", {
      name: "Check connection",
    }),
  );
  await waitFor(() => {
    expect(screen.getByRole("status").textContent).toContain("Runtime ready");
  });
  await user.click(
    screen.getByRole("button", {
      name: "Add node",
    }),
  );
  expect(pairSsh).toHaveBeenCalledWith(
    "Remote SSH",
    "remote-host-changed",
    "/private/runtime/node.sock",
    {
      identity: {
        environmentId: "remote",
        storeId: "store",
        ownerScopeId: "owner",
      },
      incarnationId: "live",
    },
  );
  await waitFor(() => {
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
it("keeps SSH enrollment unavailable when installation or runtime readiness is missing", async () => {
  const { inspectSsh, pairSsh } = setup();
  inspectSsh.mockResolvedValue(
    readiness({
      status: "not-installed",
      nextAction: "install-runtime",
      installed: false,
      ready: false,
    }),
  );
  const user = userEvent.setup();
  await openAdd(user);
  await user.type(screen.getByLabelText("Connection name"), "Remote");
  await user.selectOptions(screen.getByLabelText("Connection method"), "ssh");
  await user.type(screen.getByLabelText("SSH host"), "remote-host");
  await user.click(screen.getByText("Advanced"));
  await user.type(screen.getByLabelText("Runtime socket path"), "/runtime/node.sock");
  await user.click(
    screen.getByRole("button", {
      name: "Check connection",
    }),
  );
  await waitFor(() => {
    expect(screen.getByRole("status").textContent).toContain(
      "Install Swarmie runtime on the remote computer",
    );
  });
  expect(
    fixtureControl(
      screen.getByRole("button", {
        name: "Add node",
      }),
    ).disabled,
  ).toBe(true);
  expect(pairSsh).not.toHaveBeenCalled();
});
it("offers SSH before any node is enrolled and opens its account-free flow directly", async () => {
  setup();
  const user = userEvent.setup();
  await user.click(
    screen.getByRole("tab", {
      name: "SSH",
    }),
  );
  expect(screen.getByText("No devices added yet")).toBeTruthy();
  await user.click(
    screen.getByRole("button", {
      name: "Add",
    }),
  );
  expect(fixtureControl(screen.getByLabelText("Connection method")).value).toBe("ssh");
  expect(screen.getByLabelText("SSH host")).toBeTruthy();
  expect(screen.getByText(/No account is required/u)).toBeTruthy();
  expect(screen.queryByLabelText("Access token")).toBeNull();
});
const guidance: [SshReadiness["status"], string][] = [
  ["ssh-client-unavailable", "Install OpenSSH on this computer"],
  ["ssh-unavailable", "Check the SSH host, network"],
  ["invalid-installation", "Check the remote Swarmie installation"],
  ["runtime-unavailable", "runtime service is running on the remote computer"],
  ["incompatible-runtime", "remote runtime could not be verified"],
];
it.each(guidance)(
  "shows actionable %s guidance without offering to enroll an unverified node",
  async (status, message) => {
    const { inspectSsh, pairSsh } = setup();
    inspectSsh.mockResolvedValue(
      readiness({
        status,
        installed: null,
        ready: false,
      }),
    );
    const user = userEvent.setup();
    await user.click(
      screen.getByRole("tab", {
        name: "SSH",
      }),
    );
    await user.click(
      screen.getByRole("button", {
        name: "Add",
      }),
    );
    await user.type(screen.getByLabelText("Connection name"), "Remote");
    await user.type(screen.getByLabelText("SSH host"), "server");
    await user.click(
      screen.getByRole("button", {
        name: "Check connection",
      }),
    );
    await waitFor(() => {
      expect(screen.getByRole("status").textContent).toContain(message);
    });
    expect(
      fixtureControl(
        screen.getByRole("button", {
          name: "Add node",
        }),
      ).disabled,
    ).toBe(true);
    expect(pairSsh).not.toHaveBeenCalled();
  },
);
it("installs and connects only after an explicit action and keeps the dialog locked during preparation", async () => {
  const { inspectSsh, prepareSsh, pairSsh } = setup();
  inspectSsh.mockResolvedValue(
    readiness({
      status: "not-installed",
      installed: false,
      ready: false,
      preparation: {
        available: true,
        action: "install",
        platform: "linux-x86_64",
        installPath: "/home/user/.local/share/swarmie/runtime",
      },
    }),
  );
  const user = userEvent.setup();
  await user.click(
    screen.getByRole("tab", {
      name: "SSH",
    }),
  );
  await user.click(
    screen.getByRole("button", {
      name: "Add",
    }),
  );
  await user.type(screen.getByLabelText("Connection name"), "Work");
  await user.type(screen.getByLabelText("SSH host"), "server");
  await user.click(
    screen.getByRole("button", {
      name: "Check connection",
    }),
  );
  expect(prepareSsh).not.toHaveBeenCalled();
  expect(screen.getByText(/Install the runtime for your remote user/u).textContent).toContain(
    "/home/user/.local/share/swarmie/runtime",
  );
  const finishGate = deferred<SshReadiness>();
  const finish = finishGate.resolve;
  prepareSsh.mockImplementation(async () => await finishGate.request());
  await user.click(
    screen.getByRole("button", {
      name: "Install and connect",
    }),
  );
  await user.keyboard("{Escape}{Enter}");
  expect(screen.getByRole("dialog")).toBeTruthy();
  expect(prepareSsh).toHaveBeenCalledExactlyOnceWith("Work", "server", "", "install");
  expect(pairSsh).not.toHaveBeenCalled();
  const target = {
    identity: {
      environmentId: "prepared",
      storeId: "store",
      ownerScopeId: "owner",
    },
    incarnationId: "live",
  };
  finish(
    readiness({
      status: "ready",
      ready: true,
      runtimeIdentity: target,
    }),
  );
  expect(pairSsh).not.toHaveBeenCalled();
  await waitFor(() => {
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
it("offers start separately and requires a new check after uncertain preparation failure", async () => {
  const { inspectSsh, prepareSsh, pairSsh } = setup();
  inspectSsh.mockResolvedValue(
    readiness({
      status: "runtime-unavailable",
      installed: true,
      ready: false,
      preparation: {
        available: true,
        action: "start",
      },
    }),
  );
  prepareSsh.mockRejectedValue(new Error("Connection lost"));
  const user = userEvent.setup();
  await user.click(
    screen.getByRole("tab", {
      name: "SSH",
    }),
  );
  await user.click(
    screen.getByRole("button", {
      name: "Add",
    }),
  );
  await user.type(screen.getByLabelText("Connection name"), "Work");
  await user.type(screen.getByLabelText("SSH host"), "server");
  await user.click(
    screen.getByRole("button", {
      name: "Check connection",
    }),
  );
  await user.click(
    screen.getByRole("button", {
      name: "Start and connect",
    }),
  );
  await waitFor(() => {
    expect(screen.getByRole("alert").textContent).toContain(
      "remote preparation may have completed",
    );
  });
  expect(prepareSsh).toHaveBeenCalledExactlyOnceWith("Work", "server", "", "start");
  expect(pairSsh).not.toHaveBeenCalled();
  expect(
    screen.queryByRole("button", {
      name: "Start and connect",
    }),
  ).toBeNull();
  expect(
    fixtureControl(
      screen.getByRole("button", {
        name: "Add node",
      }),
    ).disabled,
  ).toBe(true);
});
it("explains unavailable preparation without offering to modify the remote host", async () => {
  const { inspectSsh, prepareSsh } = setup();
  inspectSsh.mockResolvedValue(
    readiness({
      status: "not-installed",
      installed: false,
      ready: false,
      preparation: {
        available: false,
        reason: "No runtime release source is configured for this platform.",
      },
    }),
  );
  const user = userEvent.setup();
  await user.click(
    screen.getByRole("tab", {
      name: "SSH",
    }),
  );
  await user.click(
    screen.getByRole("button", {
      name: "Add",
    }),
  );
  await user.type(screen.getByLabelText("SSH host"), "server");
  await user.click(
    screen.getByRole("button", {
      name: "Check connection",
    }),
  );
  expect(
    screen.getByText("No runtime release source is configured for this platform."),
  ).toBeTruthy();
  expect(
    screen.queryByRole("button", {
      name: "Install and connect",
    }),
  ).toBeNull();
  expect(prepareSsh).not.toHaveBeenCalled();
});

function readiness(patch: Partial<SshReadiness>): SshReadiness {
  return {
    status: "runtime-unavailable",
    installed: false,
    ready: false,
    nextAction: "check-runtime",
    ...patch,
  };
}
