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
  const client = { pair, removeRoute: vi.fn().mockResolvedValue(undefined), refresh: vi.fn().mockResolvedValue(undefined), localSnapshot: () => snapshot, nodeSnapshot: () => snapshot, probe: vi.fn(), status: () => "Connected", error: () => "", select: vi.fn(), connect: vi.fn(), disconnect: vi.fn(), reconnect: vi.fn().mockResolvedValue(undefined) };
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
