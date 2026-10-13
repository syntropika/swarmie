// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { NodeSettings } from "./NodeSettings";
import type { NodeClient, NodesState } from "./nodes";
afterEach(cleanup);
function setup(fail = false) {
  const pair = fail ? vi.fn().mockRejectedValue(new Error("Node identity could not be verified")) : vi.fn().mockResolvedValue(undefined);
  const snapshot = { connected: true, harnesses: [], error: "", node: { environmentId: "local" } };
  const nodes = { nodes: [
    { identity: { environmentId: "local" }, displayName: "This computer", local: true, registered: true, connections: [], lastVerifiedAtMs: 0 },
    { identity: { environmentId: "remote" }, displayName: "Remote", local: false, registered: true, lastVerifiedAtMs: 0, connections: [{ name: "main", revision: 7, transport: { kind: "http", url: "http://remote.invalid" } }] },
  ], error: "", store: { getSnapshot: () => snapshot } } as unknown as NodesState;
  const client = { pair, inspectSsh: vi.fn().mockResolvedValue({ status: "ready", nextAction: "connect", installed: true, ready: true, version: "swarmie 0.1.0", runtimeIdentity: { identity: { environmentId: "remote", storeId: "store", ownerScopeId: "owner" }, incarnationId: "live" } }), pairSsh: vi.fn().mockResolvedValue(undefined), removeRoute: vi.fn().mockResolvedValue(undefined), refresh: vi.fn().mockResolvedValue(undefined), localSnapshot: () => snapshot, nodeSnapshot: () => snapshot, probe: vi.fn(), status: () => "Connected", error: () => "", select: vi.fn(), connect: vi.fn(), disconnect: vi.fn(), reconnect: vi.fn().mockResolvedValue(undefined) };
  render(<NodeSettings client={client as unknown as NodeClient} nodes={nodes} />);
  return { ...client, snapshot };
}
async function openAdd(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("tab", { name: "Control other devices" }));
  await user.click(screen.getByRole("button", { name: "Add" }));
}
it("keeps all connections inside one accessible view selector with keyboard navigation", async () => {
  setup(); const user = userEvent.setup();
  expect(screen.getByRole("tabpanel").textContent).toContain("This computer");
  expect(screen.queryByRole("switch")).toBeNull();
  await user.click(screen.getByRole("tab", { name: "Control this computer" }));
  await user.keyboard("{ArrowRight}");
  expect(document.activeElement).toBe(screen.getByRole("tab", { name: "Control other devices" }));
  expect(screen.getByRole("tabpanel").textContent).toContain("Remote");
  expect(screen.getByRole("tab", { name: "Control other devices" }).getAttribute("aria-selected")).toBe("true");
  await user.keyboard("{Home}");
  expect(screen.queryByRole("switch")).toBeNull();
});
it("submits from the keyboard and preserves inputs on failed verification, then clears canceled credentials", async () => {
  const { pair } = setup(true); const user = userEvent.setup(); await openAdd(user);
  await user.type(screen.getByLabelText("Connection name"), "Remote route"); await user.type(screen.getByLabelText("Address"), "http://localhost:7490");
  await user.type(screen.getByLabelText("Access token"), "secret"); await user.keyboard("{Enter}");
  await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("could not be verified"));
  expect(pair).toHaveBeenCalledWith("Remote route", "http://localhost:7490", "secret");
  expect((screen.getByLabelText("Address") as HTMLInputElement).value).toBe("http://localhost:7490");
  await user.click(screen.getByRole("button", { name: "Cancel" }));
  expect(document.activeElement).toBe(screen.getByRole("button", { name: "Add" }));
  await user.click(screen.getByRole("button", { name: "Add" }));
  expect((screen.getByLabelText("Access token") as HTMLInputElement).value).toBe("");
});
it("closes a successful add dialog and prevents dismissal or repeat submission during verification", async () => {
  const { pair } = setup(); const user = userEvent.setup(); let finish!: () => void;
  pair.mockImplementation(() => new Promise<void>((resolve) => { finish = resolve; }));
  await openAdd(user); await user.type(screen.getByLabelText("Connection name"), "Route"); await user.type(screen.getByLabelText("Address"), "http://localhost:7490");
  await user.keyboard("{Enter}{Escape}{Enter}");
  expect(screen.getByRole("dialog")).toBeTruthy(); expect(pair).toHaveBeenCalledOnce();
  expect((screen.getByRole("button", { name: "Cancel" }) as HTMLButtonElement).disabled).toBe(true);
  finish(); await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
});
it("removes only the route revision in connection details", async () => {
  const { removeRoute } = setup(); const user = userEvent.setup();
  await user.click(screen.getByRole("tab", { name: "Control other devices" }));
  await user.click(screen.getByRole("button", { name: "Remote actions" })); await user.click(screen.getByRole("menuitem", { name: "Details" }));
  await user.click(screen.getByRole("button", { name: "Remove connection main" })); expect(removeRoute).toHaveBeenCalledWith("main", 7);
});
it("lets a remote connection stop observation without exposing a local disconnect control", async () => {
  const { disconnect } = setup(); const user = userEvent.setup();
  expect(screen.queryByRole("switch")).toBeNull();
  await user.click(screen.getByRole("button", { name: "This computer actions" }));
  expect(within(screen.getByRole("menu")).queryByText(/connect/i)).toBeNull();
  await user.keyboard("{Escape}"); await user.click(screen.getByRole("tab", { name: "Control other devices" }));
  await user.click(screen.getByRole("switch", { name: "Connect Remote" }));
  expect(disconnect).toHaveBeenCalledWith("remote");
});
it("shows authentication recovery for the automatic local connection", async () => {
  const { snapshot, reconnect } = setup(); const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: "This computer actions" })); await user.click(screen.getByRole("menuitem", { name: "Details" }));
  await user.click(screen.getByRole("button", { name: "Connection access" }));
  snapshot.connected = false; snapshot.error = "Token rejected";
  await user.type(screen.getByLabelText("Local access token"), "secret"); await user.keyboard("{Enter}");
  expect(reconnect).toHaveBeenCalledWith("secret"); await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("Token rejected"));
  expect(screen.getByRole("dialog")).toBeTruthy();
});

it("checks SSH readiness before pairing and clears a prepared target when connection inputs change", async () => {
  const { inspectSsh, pairSsh } = setup(); const user = userEvent.setup(); await openAdd(user);
  await user.type(screen.getByLabelText("Connection name"), "Remote SSH");
  await user.selectOptions(screen.getByLabelText("Connection method"), "ssh");
  await user.type(screen.getByLabelText("SSH host"), "remote-host");
  await user.click(screen.getByText("Advanced"));
  await user.type(screen.getByLabelText("Runtime socket path"), "/private/runtime/node.sock");
  expect((screen.getByRole("button", { name: "Add node" }) as HTMLButtonElement).disabled).toBe(true);
  await user.click(screen.getByRole("button", { name: "Check connection" }));
  await waitFor(() => expect(screen.getByRole("status").textContent).toContain("Runtime ready"));
  expect(inspectSsh).toHaveBeenCalledWith("remote-host", "/private/runtime/node.sock");
  await user.type(screen.getByLabelText("SSH host"), "-changed");
  expect((screen.getByRole("button", { name: "Add node" }) as HTMLButtonElement).disabled).toBe(true);
  await user.click(screen.getByRole("button", { name: "Check connection" }));
  await waitFor(() => expect(screen.getByRole("status").textContent).toContain("Runtime ready"));
  await user.click(screen.getByRole("button", { name: "Add node" }));
  expect(pairSsh).toHaveBeenCalledWith("Remote SSH", "remote-host-changed", "/private/runtime/node.sock", { identity: { environmentId: "remote", storeId: "store", ownerScopeId: "owner" }, incarnationId: "live" });
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
});
it("keeps SSH enrollment unavailable when installation or runtime readiness is missing", async () => {
  const { inspectSsh, pairSsh } = setup(); inspectSsh.mockResolvedValue({ status: "not-installed", nextAction: "install-runtime", installed: false, ready: false } as never);
  const user = userEvent.setup(); await openAdd(user);
  await user.type(screen.getByLabelText("Connection name"), "Remote"); await user.selectOptions(screen.getByLabelText("Connection method"), "ssh");
  await user.type(screen.getByLabelText("SSH host"), "remote-host"); await user.click(screen.getByText("Advanced")); await user.type(screen.getByLabelText("Runtime socket path"), "/runtime/node.sock");
  await user.click(screen.getByRole("button", { name: "Check connection" }));
  await waitFor(() => expect(screen.getByRole("status").textContent).toContain("Install Swarmie runtime on the remote computer"));
  expect((screen.getByRole("button", { name: "Add node" }) as HTMLButtonElement).disabled).toBe(true);
  expect(pairSsh).not.toHaveBeenCalled();
});

it("offers SSH before any node is enrolled and opens its account-free flow directly", async () => {
  setup(); const user = userEvent.setup();
  await user.click(screen.getByRole("tab", { name: "SSH" }));
  expect(screen.getByText("No devices added yet")).toBeTruthy();
  await user.click(screen.getByRole("button", { name: "Add" }));
  expect((screen.getByLabelText("Connection method") as HTMLSelectElement).value).toBe("ssh");
  expect(screen.getByLabelText("SSH host")).toBeTruthy();
  expect(screen.getByText(/No account is required/)).toBeTruthy();
  expect(screen.queryByLabelText("Access token")).toBeNull();
});
it.each([
  ["ssh-client-unavailable", "Install OpenSSH on this computer"],
  ["ssh-unavailable", "Check the SSH host, network"],
  ["invalid-installation", "Check the remote Swarmie installation"],
  ["runtime-unavailable", "runtime service is running on the remote computer"],
  ["incompatible-runtime", "remote runtime could not be verified"],
])("shows actionable %s guidance without offering to enroll an unverified node", async (status, message) => {
  const { inspectSsh, pairSsh } = setup();
  inspectSsh.mockResolvedValue({ status, installed: null, ready: false } as never);
  const user = userEvent.setup(); await user.click(screen.getByRole("tab", { name: "SSH" }));
  await user.click(screen.getByRole("button", { name: "Add" }));
  await user.type(screen.getByLabelText("Connection name"), "Remote");
  await user.type(screen.getByLabelText("SSH host"), "server");
  await user.click(screen.getByRole("button", { name: "Check connection" }));
  await waitFor(() => expect(screen.getByRole("status").textContent).toContain(message));
  expect((screen.getByRole("button", { name: "Add node" }) as HTMLButtonElement).disabled).toBe(true);
  expect(pairSsh).not.toHaveBeenCalled();
});
