import type { McpCatalog, McpEntry } from "@swarmie/contracts";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
// @vitest-environment happy-dom
import { cleanup, render as testingRender, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactNode } from "react";
import { afterEach, expect, it, vi } from "vitest";

import { McpSettings } from "~/web/src/mcp-settings";
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
const entry: McpEntry = {
  id: "exact-id",
  name: "fixture",
  nativeName: "fixture",
  origin: "native",
  revision: "revision-1",
  harness: "codex",
  scope: "project",
  source: "/project/.codex/config.toml",
  projectPath: "/project",
  enabled: true,
  definition: {
    transport: "stdio",
    command: "server",
    args: [],
    envVars: [],
    url: null,
    headerEnvVars: {},
    bearerTokenEnvVar: null,
  },
  actions: ["update", "enable", "disable", "uninstall"],
  credentialFields: [],
  readOnlyReason: null,
  runtimeStatus: "connected",
  authStatus: "unsupported",
  tools: ["fixture_read"],
  requiresRestart: false,
};
const catalog = (entries = [entry]): McpCatalog => ({
  entries,
  limitations: [],
  restartSessions: [],
});
function fixture() {
  const rpc = vi.fn().mockResolvedValue(catalog());
  const store = new WorkspaceStore();
  vi.spyOn(store, "rpc").mockImplementation(rpc);
  vi.spyOn(store, "restoreThread").mockResolvedValue(undefined);
  return { rpc, store, state: store.getSnapshot() };
}

it("toggles the exact native origin using its observed revision", async () => {
  const f = fixture();
  render(<McpSettings store={f.store} state={f.state} />);
  f.rpc.mockImplementation(async (method) =>
    method === "mcp/list"
      ? { ...catalog(), restartSessions: ["session"] }
      : { action: "disable", entry, restartSessions: ["session"], authorizationUrl: null },
  );
  await userEvent.setup().click(await screen.findByRole("switch", { name: "Enable fixture" }));
  await waitFor(() => {
    expect(f.rpc).toHaveBeenCalledWith(
      "mcp/mutate",
      expect.objectContaining({
        action: "disable",
        id: "exact-id",
        expectedRevision: "revision-1",
        query: {},
      }),
    );
  });
  expect(await screen.findByText(/Reopen the affected native sessions/u)).toBeTruthy();
  expect(screen.getByRole("button", { name: "Reopen session" })).toBeTruthy();
});

it("registers a command using credential variable names and confirms removal", async () => {
  const f = fixture();
  f.rpc.mockResolvedValue(catalog([]));
  render(<McpSettings store={f.store} state={f.state} />);
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "Add MCP" }));
  await user.type(screen.getByLabelText("MCP server name"), "shared-server");
  await user.type(screen.getByLabelText("MCP command"), "fixture-server");
  await user.type(screen.getByLabelText("MCP environment variables"), "MCP_TOKEN");
  f.rpc.mockImplementation(async (method) =>
    method === "mcp/list"
      ? catalog()
      : { action: "install", entry, restartSessions: [], authorizationUrl: null },
  );
  await user.click(screen.getByRole("button", { name: "Install MCP" }));
  await waitFor(() => {
    expect(f.rpc).toHaveBeenCalledWith(
      "mcp/mutate",
      expect.objectContaining({
        action: "install",
        origin: "swarmie",
        name: "shared-server",
        definition: {
          transport: "stdio",
          command: "fixture-server",
          args: [],
          envVars: ["MCP_TOKEN"],
        },
      }),
    );
  });
  await user.click(await screen.findByRole("button", { name: "Uninstall" }));
  expect(f.rpc).not.toHaveBeenCalledWith(
    "mcp/mutate",
    expect.objectContaining({ action: "uninstall" }),
  );
  await user.click(screen.getByRole("button", { name: "Confirm uninstall" }));
  await waitFor(() => {
    expect(f.rpc).toHaveBeenCalledWith(
      "mcp/mutate",
      expect.objectContaining({
        action: "uninstall",
        id: entry.id,
        expectedRevision: entry.revision,
      }),
    );
  });
});

it("hides stale catalogs and editors when switching execution nodes", async () => {
  const first = fixture();
  const next = fixture();
  const pending = deferred<unknown>();
  next.rpc.mockImplementation(async () => await pending.promise);
  const view = render(<McpSettings store={first.store} state={first.state} />);
  await screen.findByText("fixture");
  await userEvent.setup().click(screen.getByRole("button", { name: "Edit" }));
  view.rerender(<McpSettings store={next.store} state={next.state} />);
  expect(screen.queryByRole("form", { name: "MCP server editor" })).toBeNull();
  expect(screen.queryByText("fixture")).toBeNull();
  pending.resolve(catalog([{ ...entry, name: "next-node" }]));
  await screen.findByText("next-node");
  expect(first.rpc.mock.calls.every((call) => call[0] === "mcp/list")).toBe(true);
});

it("retains a failed edit and reports a stale revision without claiming success", async () => {
  const f = fixture();
  render(<McpSettings store={f.store} state={f.state} />);
  await screen.findByText("fixture");
  await userEvent.setup().click(screen.getByRole("button", { name: "Edit" }));
  f.rpc.mockRejectedValue(new Error("Native configuration changed; refresh"));
  await userEvent.setup().click(screen.getByRole("button", { name: "Save changes" }));
  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toContain("Native configuration changed");
  expect(screen.getByRole("form", { name: "MCP server editor" })).toBeTruthy();
  expect(screen.queryByText("MCP settings saved.")).toBeNull();
});

it("retains a successful save acknowledgement when only the subsequent inventory refresh fails", async () => {
  const f = fixture();
  render(<McpSettings store={f.store} state={f.state} />);
  const toggle = await screen.findByRole("switch", { name: "Enable fixture" });
  f.rpc.mockImplementation(async (method) => {
    if (method === "mcp/mutate") {
      return { action: "disable", entry, restartSessions: [], authorizationUrl: null };
    }
    throw new Error("Inventory refresh failed");
  });
  await userEvent.setup().click(toggle);
  await screen.findByText("MCP settings saved.");
  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toContain("Inventory refresh failed");
});
