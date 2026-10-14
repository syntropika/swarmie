import type { RuntimeIdentity } from "@swarmie/contracts";
// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { WorkspaceCache } from "./cache";
import { NodeClient } from "./nodes";
import type { SshReadiness } from "./nodes";
import { fixtureRequestBody } from "./test-fixtures";
import { WorkspaceStore } from "./workspace";

const local: RuntimeIdentity = {
  identity: {
    environmentId: "local",
    storeId: "local-store",
    ownerScopeId: "owner",
  },
  incarnationId: "local-runtime",
};
const remote: RuntimeIdentity = {
  identity: {
    environmentId: "remote",
    storeId: "remote-store",
    ownerScopeId: "owner",
  },
  incarnationId: "inspected-runtime",
};
let requests: {
  path: string;
  method: string;
  params: Record<string, unknown>;
  expectedRuntime?: RuntimeIdentity;
}[];
let readiness: SshReadiness;
let currentLocal: RuntimeIdentity;
let pairError: string | undefined;
let client: NodeClient;
beforeEach(() => {
  requests = [];
  readiness = {
    status: "ready",
    nextAction: "connect",
    installed: true,
    ready: true,
    version: "swarmie 0.1.0",
    runtimeIdentity: remote,
  };
  currentLocal = structuredClone(local);
  pairError = undefined;
  localStorage.clear();
  vi.spyOn(WorkspaceCache.prototype, "load").mockResolvedValue(undefined);
  vi.spyOn(WorkspaceCache.prototype, "save").mockResolvedValue();
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options: RequestInit) => {
      const request = fixtureRequestBody(options);
      requests.push({
        path,
        ...request,
      });
      const error =
        request.method === "nodes/pairSsh" && typeof pairError === "string" && pairError.length > 0
          ? {
              code: -32_005,
              message: pairError,
            }
          : undefined;
      const result = (() => {
        if (request.method === "runtime/initialize") {
          return {
            name: "swarmie",
            protocolVersion: "1",
            runtimeIdentity: currentLocal,
          };
        }
        if (["nodes/inspectSsh", "nodes/prepareSsh"].includes(request.method)) {
          return readiness;
        }
        if (request.method === "nodes/list") {
          return {
            nodes: [],
          };
        }
        return null;
      })();
      return Response.json({
        jsonrpc: "2.0",
        id: request.id,
        ...(error
          ? {
              error,
            }
          : {
              result,
            }),
      });
    }),
  );
  client = new NodeClient(
    new WorkspaceStore({
      node: local.identity,
    }),
  );
});
afterEach(() => {
  client.dispose();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  localStorage.clear();
});
it("sends the inspected remote identity to native enrollment through the verified local runtime", async () => {
  const inspected = await client.inspectSsh("ssh-alias", "/runtime/node.sock");
  await client.pairSsh(
    "Work node",
    "ssh-alias",
    "/runtime/node.sock",
    requireFixtureValue(inspected.runtimeIdentity),
  );
  expect(requests.find((request) => request.method === "nodes/inspectSsh")).toMatchObject({
    path: "/client/rpc",
    params: {
      host: "ssh-alias",
      socketPath: "/runtime/node.sock",
    },
    expectedRuntime: local,
  });
  expect(requests.find((request) => request.method === "nodes/pairSsh")).toMatchObject({
    path: "/client/rpc",
    params: {
      name: "Work node",
      host: "ssh-alias",
      socketPath: "/runtime/node.sock",
      runtimeIdentity: remote,
    },
    expectedRuntime: local,
  });
  expect(
    requests
      .filter((request) => request.method.startsWith("nodes/"))
      .map((request) => request.method),
  ).toEqual(["nodes/inspectSsh", "nodes/pairSsh", "nodes/list"]);
  expect(requests.every((request) => request.path.startsWith("/"))).toBe(true);
});
it("preserves unavailable runtime guidance without enrolling or changing the catalog", async () => {
  readiness = {
    status: "not-installed",
    installed: false,
    ready: false,
    nextAction: "install_runtime",
  };
  expect(await client.inspectSsh("ssh-alias", "/runtime/node.sock")).toEqual(readiness);
  expect(requests.map((request) => request.method)).toEqual([
    "runtime/initialize",
    "nodes/inspectSsh",
  ]);
  expect(client.getSnapshot().nodes).toEqual([]);
  readiness = {
    status: "runtime-unavailable",
    installed: true,
    ready: false,
    version: "swarmie 0.1.0",
    nextAction: "start_runtime_or_check_socket",
  };
  expect(await client.inspectSsh("ssh-alias", "/runtime/node.sock")).toEqual(readiness);
  expect(requests.some((request) => request.method === "nodes/pairSsh")).toBe(false);
});
it("propagates replacement of the inspected remote without refreshing or selecting a node", async () => {
  const inspected = await client.inspectSsh("ssh-alias", "/runtime/node.sock");
  pairError = "The runtime changed after SSH inspection. Check the connection again.";
  await expect(
    client.pairSsh(
      "Work node",
      "ssh-alias",
      "/runtime/node.sock",
      requireFixtureValue(inspected.runtimeIdentity),
    ),
  ).rejects.toThrow("changed after SSH inspection");
  expect(
    requests.find((request) => request.method === "nodes/pairSsh")?.params.runtimeIdentity,
  ).toEqual(remote);
  expect(
    requests.some((request) =>
      ["nodes/list", "nodes/select", "session/prompt"].includes(request.method),
    ),
  ).toBe(false);
  expect(client.getSnapshot().nodes).toEqual([]);
});
it("rejects local node replacement before sending the remote enrollment command", async () => {
  const inspected = await client.inspectSsh("ssh-alias", "/runtime/node.sock");
  currentLocal = {
    ...local,
    identity: {
      ...local.identity,
      storeId: "replacement-store",
    },
  };
  await expect(
    client.pairSsh(
      "Work node",
      "ssh-alias",
      "/runtime/node.sock",
      requireFixtureValue(inspected.runtimeIdentity),
    ),
  ).rejects.toThrow("different node");
  expect(requests.some((request) => request.method === "nodes/pairSsh")).toBe(false);
});
it("dispatches explicit preparation once against the verified local node", async () => {
  expect(await client.prepareSsh("Work", "ssh-alias", "", "install")).toEqual(readiness);
  expect(requests.map((request) => request.method)).toEqual([
    "runtime/initialize",
    "nodes/prepareSsh",
    "runtime/initialize",
    "nodes/list",
  ]);
  expect(requests[1]).toMatchObject({
    params: {
      name: "Work",
      host: "ssh-alias",
      socketPath: "",
      action: "install",
    },
    expectedRuntime: local,
  });
});
function requireFixtureValue<T>(value: T | null | undefined): T {
  if (value === undefined || value === null) {
    throw new Error("Expected the fixture value to be present.");
  }
  return value;
}
