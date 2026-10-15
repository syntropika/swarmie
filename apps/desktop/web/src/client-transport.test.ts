import { afterEach, expect, it, vi } from "vitest";

import { record, responseResult } from "./checked-json";
import { clientRequest, nativeClient, observeClient } from "./client-transport";
import { fixtureText } from "./test-fixtures";

afterEach(() => vi.unstubAllGlobals());
function native(invoke: (command: string, payload: Record<string, unknown>) => Promise<unknown>) {
  vi.stubGlobal("window", {
    location: {
      search: "?client=native",
    },
    zero: {
      invoke,
    },
  });
}
it("uses the native application bridge without exposing connection credentials or fetching RPC", async () => {
  const fetch = vi.fn();
  vi.stubGlobal("fetch", fetch);
  const invoke = vi.fn(async () => ({
    response: {
      jsonrpc: "2.0",
      id: 1,
      result: "done",
    },
  }));
  native(invoke);
  const response = await clientRequest(
    "/rpc",
    {
      jsonrpc: "2.0",
      id: 1,
      method: "project/list",
    },
    "private-token",
    1000,
  );
  const envelope: unknown = await response.json();
  expect(responseResult(envelope)).toBe("done");
  expect(invoke.mock.calls[0]).toEqual([
    "swarmie.client",
    {
      operation: "rpc",
      path: "/rpc",
      request: {
        jsonrpc: "2.0",
        id: 1,
        method: "project/list",
      },
    },
  ]);
  expect(fetch).not.toHaveBeenCalled();
});
it("does not fall back to HTTP or retry after a native dispatch failure", async () => {
  const fetch = vi.fn();
  vi.stubGlobal("fetch", fetch);
  const invoke = vi.fn(async () => {
    throw new Error("delivery uncertain");
  });
  native(invoke);
  await expect(
    clientRequest(
      "/rpc",
      {
        method: "session/prompt",
      },
      "",
      1000,
    ),
  ).rejects.toThrow("delivery uncertain");
  expect(invoke).toHaveBeenCalledTimes(1);
  expect(fetch).not.toHaveBeenCalled();
});
it("stages large Unicode requests once and reconstructs chunked responses", async () => {
  const staged: string[] = [];
  let dispatches = 0;
  let released = false;
  const result = JSON.stringify({
    jsonrpc: "2.0",
    id: 9,
    result: "🦀".repeat(100_000),
  });
  native(async (_command, payload) => {
    if (payload.operation === "stage") {
      expect(payload.index).toBe(staged.length);
      staged.push(fixtureText(payload.chunk));
      return {
        transferId: "request",
      };
    }
    if (payload.operation === "dispatch") {
      dispatches += 1;
      const stagedRequest: unknown = JSON.parse(staged.join(""));
      expect(record(record(stagedRequest).params).text).toBe("🦀".repeat(100_000));
      return {
        transferId: "response",
        chunks: 2,
      };
    }
    if (payload.operation === "read") {
      return {
        chunk: payload.index === 0 ? result.slice(0, 100_000) : result.slice(100_000),
      };
    }
    if (payload.operation === "release") {
      released = true;
      return null;
    }
    throw new Error("unexpected operation");
  });
  const response = await clientRequest(
    "/rpc",
    {
      jsonrpc: "2.0",
      id: 9,
      params: {
        text: "🦀".repeat(100_000),
      },
    },
    "",
    1000,
  );
  const envelope: unknown = await response.json();
  expect(responseResult(envelope)).toBe("🦀".repeat(100_000));
  expect(dispatches).toBe(1);
  expect(released).toBe(true);
});
it("receives client observations through IPC and keeps plain browser access on HTTP", async () => {
  const invoke = vi.fn(async () => ({
    events: [
      {
        method: "client/connected",
      },
    ],
  }));
  native(invoke);
  const observation = await observeClient();
  expect(observation.events[0].method).toBe("client/connected");
  vi.stubGlobal("window", {
    location: {
      search: "",
    },
    zero: {
      invoke,
    },
  });
  expect(nativeClient()).toBe(false);
  const fetch = vi.fn(async () => new Response("{}"));
  vi.stubGlobal("fetch", fetch);
  await clientRequest(
    "/rpc",
    {
      id: 1,
    },
    "browser-token",
    1000,
  );
  expect(fetch).toHaveBeenCalledOnce();
});
