import type { Node } from "@swarmie/plugin-connections-browser";

type Descriptor = Omit<Node, "url"> & { url: string };

function nonempty(value: unknown): value is string {
  return typeof value === "string" && value.trim() !== "";
}

function descriptor(value: unknown): value is Descriptor {
  return (
    typeof value === "object" &&
    value !== null &&
    "id" in value &&
    nonempty(value.id) &&
    new TextEncoder().encode(value.id).length <= 128 &&
    !/\s/u.test(value.id) &&
    "label" in value &&
    nonempty(value.label) &&
    "principal" in value &&
    nonempty(value.principal) &&
    "url" in value &&
    typeof value.url === "string"
  );
}

/** An embedding may publish a public descriptor beside the client application. */
export async function browserNode(location: string): Promise<Node | null> {
  const response = await fetch(new URL("/__swarmie/node", location), {
    headers: { Accept: "application/json" },
    signal: AbortSignal.timeout(2000),
    cache: "no-store",
  });
  if (response.status === 404 || response.status === 204) {
    return null;
  }
  if (!response.ok) {
    throw new Error("The application node descriptor is unavailable.");
  }
  if (!(response.headers.get("content-type")?.includes("application/json") ?? false)) {
    return null;
  }
  const value: unknown = await response.json();
  if (!descriptor(value)) {
    throw new Error("The application node descriptor is invalid.");
  }
  const url = new URL(value.url, location);
  if (value.url.startsWith("/") && !value.url.startsWith("//")) {
    url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  }
  if (
    (url.protocol !== "ws:" && url.protocol !== "wss:") ||
    url.username !== "" ||
    url.password !== "" ||
    url.hash !== ""
  ) {
    throw new Error(
      "The application node requires a WebSocket URL without credentials or fragment.",
    );
  }
  return { id: value.id, label: value.label, principal: value.principal, url: url.href };
}
