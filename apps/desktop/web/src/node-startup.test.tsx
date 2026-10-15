import { cleanup, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";

import { NodeStartupControls } from "~/web/src/node-startup-controls";
import { NodeClient } from "~/web/src/nodes";
import type { ClientNode, NodeStartup } from "~/web/src/nodes";
import { renderWithQuery as render } from "~/web/src/test-fixtures";

afterEach(cleanup);
const node: ClientNode = {
  identity: {
    environmentId: "local",
    storeId: "store",
    ownerScopeId: "owner",
  },
  displayName: "Local",
  local: true,
  registered: true,
  connections: [],
  lastVerifiedAtMs: 0,
};
const observation: NodeStartup = {
  environmentId: "local",
  available: true,
  installed: false,
  enabled: false,
  running: false,
  trigger: "login",
  bootReady: false,
  canEnableBoot: true,
  reason: "",
  notice: "Starts at user login",
};
function setup(target = node) {
  const startup = vi.fn<NodeClient["startup"]>().mockResolvedValue(observation);
  const errors: Error[] = [];
  render(
    <NodeStartupControls
      node={target}
      client={Object.assign(new NodeClient(), { startup })}
      busy={false}
      run={async (action) => {
        try {
          await action();
        } catch (error) {
          errors.push(error instanceof Error ? error : new Error(String(error)));
        }
      }}
    />,
  );
  return {
    startup,
    errors,
    user: userEvent.setup(),
  };
}
it("changes startup only after a deliberate check and action", async () => {
  const { startup, user } = setup();
  expect(startup).not.toHaveBeenCalled();
  expect(screen.queryByRole("switch")).toBeNull();
  await user.click(
    screen.getByRole("button", {
      name: "Check startup",
    }),
  );
  expect(startup).toHaveBeenLastCalledWith(node);
  startup.mockResolvedValue({
    ...observation,
    enabled: true,
  });
  await user.click(screen.getByRole("switch"));
  expect(startup).toHaveBeenLastCalledWith(node, true, false);
  await user.click(screen.getByRole("switch"));
  expect(startup).toHaveBeenLastCalledWith(node, false, false);
});
it("requires an explicit boot action and blocks retry after an uncertain change", async () => {
  const { startup, errors, user } = setup();
  await user.click(
    screen.getByRole("button", {
      name: "Check startup",
    }),
  );
  startup.mockRejectedValueOnce(new Error("Acknowledgment lost"));
  await user.click(
    screen.getByRole("button", {
      name: "Enable startup before sign-in and after logout",
    }),
  );
  expect(startup).toHaveBeenLastCalledWith(node, true, true);
  await waitFor(() => {
    expect(errors).toHaveLength(1);
  });
  expect(errors.at(0)?.message).toContain("configuration may have changed");
  expect(screen.queryByRole("switch")).toBeNull();
  expect(screen.getByRole("status").textContent).toContain("Check the current configuration");
  await user.click(
    screen.getByRole("button", {
      name: "Check startup",
    }),
  );
  expect(screen.getByRole("switch")).toBeTruthy();
});
it("explains remote route limitations without invoking startup", () => {
  const { startup } = setup({
    ...node,
    local: false,
    connections: [
      {
        name: "direct",
        revision: 1,
        authenticated: true,
        transport: {
          kind: "http",
          url: "https://example.invalid",
        },
      },
    ],
  });
  expect(screen.queryByRole("button")).toBeNull();
  expect(screen.getByText(/through a saved SSH connection/u)).toBeTruthy();
  expect(startup).not.toHaveBeenCalled();
});
