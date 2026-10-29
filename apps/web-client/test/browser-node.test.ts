import { afterAll, expect, test } from "bun:test";

import { browserNode } from "../src/browser-node";

const descriptor = {
  id: "example-node",
  principal: "example-user",
  label: "Example node",
  url: "/rpc",
};
let response = () => Response.json(descriptor);
const server = Bun.serve({
  hostname: "127.0.0.1",
  port: 0,
  fetch(request) {
    expect(new URL(request.url).pathname).toBe("/__swarmie/node");
    expect(request.headers.get("accept")).toBe("application/json");
    return response();
  },
});
afterAll(async () => {
  await server.stop(true);
});

test("the embedding descriptor supplies a verified connection expectation and resolves its same-origin socket", async () => {
  response = () => Response.json(descriptor);
  expect(await browserNode(server.url.href)).toEqual({
    ...descriptor,
    url: `ws://${server.url.host}/rpc`,
  });
  response = () => Response.json({ ...descriptor, url: "wss://node.example.test/rpc" });
  const secure = await browserNode(server.url.href);
  expect(secure?.url).toBe("wss://node.example.test/rpc");
});

test.each([
  () => new Response(null, { status: 204 }),
  () => new Response("Not found", { status: 404 }),
  () => new Response("<!doctype html>", { headers: { "Content-Type": "text/html" } }),
])(
  "a standalone web deployment without a descriptor keeps manual node configuration available (%#)",
  async (absent) => {
    response = absent;
    expect(await browserNode(server.url.href)).toBeNull();
  },
);

test.each([
  { ...descriptor, id: "" },
  { ...descriptor, id: "two nodes" },
  { ...descriptor, id: "é".repeat(65) },
  { ...descriptor, principal: " " },
  { ...descriptor, label: "" },
  { ...descriptor, url: "https://node.example.test/rpc" },
  { ...descriptor, url: "//node.example.test/rpc" },
  { ...descriptor, url: "wss://user:password@node.example.test/rpc" },
  { ...descriptor, url: "wss://node.example.test/rpc#fragment" },
])(
  "invalid identity and transport descriptors cannot replace a verified node binding (%#)",
  (invalid) => {
    response = () => Response.json(invalid);
    expect(browserNode(server.url.href)).rejects.toThrow();
  },
);

test("descriptor failures are surfaced without synthesizing a configured node", () => {
  response = () => Response.json({ error: "Unavailable" }, { status: 503 });
  expect(browserNode(server.url.href)).rejects.toThrow("node descriptor is unavailable");
});
