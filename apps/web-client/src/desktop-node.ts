import type { Connector, Node, NodeIdentity } from "@swarmie/plugin-client-node-browser";
import { NodeProtocol, connectWebSocket } from "@swarmie/plugin-client-node-browser";
import { RemoteHost } from "@syntropika/tembra-sdk/rpc";
import type { Transport } from "@syntropika/tembra-sdk/rpc";

let desktopNodeId: string | null = null;

export function isDesktopNode(nodeId: string): boolean {
  return desktopNodeId !== null && desktopNodeId === nodeId;
}

type Native = (payload: unknown) => Promise<unknown>;
function object(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object";
}

function bridge(): Native | null {
  if (!("zero" in window) || !object(window.zero) || typeof window.zero.invoke !== "function") {
    return null;
  }
  const owner = window.zero;
  const { invoke } = owner;
  if (typeof invoke !== "function") {
    return null;
  }
  return async (payload) => {
    const response: unknown = await Reflect.apply(invoke, owner, [
      "swarmie.desktop.runtime",
      payload,
    ]);
    if (!object(response) || response.ok !== true) {
      throw new Error(
        object(response) && typeof response.error === "string"
          ? response.error
          : "The native runtime is unavailable.",
      );
    }
    return response.value;
  };
}
function identity(value: unknown): value is NodeIdentity {
  return (
    object(value) &&
    typeof value.nodeId === "string" &&
    typeof value.principal === "string" &&
    value.protocolVersion === NodeProtocol.Version
  );
}
function bytes(value: unknown): value is number[] {
  return (
    Array.isArray(value) &&
    value.length <= NodeProtocol.MaxFrame &&
    value.every(
      (byte: unknown) =>
        typeof byte === "number" && Number.isInteger(byte) && byte >= 0 && byte <= 255,
    )
  );
}
function frames(value: unknown): number[][] {
  if (!Array.isArray(value)) {
    throw new TypeError("The native runtime returned invalid transport bytes.");
  }
  return value.map((frame: unknown) => {
    if (!bytes(frame)) {
      throw new TypeError("The native runtime returned invalid transport bytes.");
    }
    return frame;
  });
}

/** The native bridge carries only Tembra transport bytes and a host-owned identity. */
export async function desktopNode(): Promise<{ node: Node; connector: Connector } | null> {
  const native = bridge();
  if (native === null) {
    return null;
  }
  const invoke: Native = native;
  const descriptor = await invoke({ action: "describe" });
  if (
    !object(descriptor) ||
    typeof descriptor.id !== "string" ||
    descriptor.principal !== "desktop-user"
  ) {
    throw new Error("The native node returned an invalid identity.");
  }
  const node: Node = {
    id: descriptor.id,
    principal: descriptor.principal,
    url: null,
    label: "This computer",
  };
  const connector: Connector = async (requested, signal, lost) => {
    if (requested.id !== node.id) {
      return await connectWebSocket(requested, signal, lost);
    }
    signal.throwIfAborted();
    const generation = await invoke({ action: "open" });
    if (typeof generation !== "number" || !Number.isSafeInteger(generation) || generation < 1) {
      throw new Error("The native channel could not open.");
    }
    let closed = false;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let sending = Promise.resolve();
    let peer: RemoteHost | null = null;
    function close() {
      if (closed) {
        return;
      }
      closed = true;
      if (timer !== null) {
        clearTimeout(timer);
      }
      signal.removeEventListener("abort", close);
      peer?.close();
      invoke?.({ action: "close", generation }).catch(() => undefined);
    }
    function failed(error: unknown) {
      if (!closed) {
        close();
        lost(error);
      }
    }
    const transport: Transport = {
      async send(frame) {
        const operation = sending.then(async () => {
          if (closed) {
            throw new Error("The native channel ended.");
          }
          await invoke({ action: "send", generation, frame: [...frame] });
        });
        sending = operation.catch(failed);
        await operation;
      },
      listen(receive, ended) {
        async function poll() {
          if (closed) {
            return;
          }
          try {
            const incoming = frames(await invoke({ action: "poll", generation }));
            if (closed) {
              return;
            }
            for (const frame of incoming) {
              receive(new Uint8Array(frame));
            }
            timer = setTimeout(() => {
              poll().catch(failed);
            }, 20);
          } catch (error: unknown) {
            ended(error);
            failed(error);
          }
        }
        poll().catch(failed);
        return () => {
          if (timer !== null) {
            clearTimeout(timer);
          }
        };
      },
      close: failed,
    };
    peer = new RemoteHost(transport, {
      maxFrame: NodeProtocol.MaxFrame,
      maxPending: NodeProtocol.MaxPending,
    });
    signal.addEventListener("abort", close, { once: true });
    try {
      const reference = await peer.get(NodeProtocol.Service, NodeProtocol.Key, { signal });
      let observed: unknown;
      try {
        const identify: unknown = Reflect.get(reference, "identify");
        if (typeof identify !== "function") {
          throw new TypeError("The native identity service is unavailable.");
        }
        observed = await Reflect.apply(identify, undefined, []);
      } finally {
        reference.release();
      }
      if (!identity(observed)) {
        throw new Error("The native node returned an invalid binding.");
      }
      const connected = peer;
      signal.throwIfAborted();
      return {
        identity: observed,
        get: async (service, key, options) => await connected.get(service, key, options),
        close,
      };
    } catch (error: unknown) {
      close();
      throw error;
    }
  };
  desktopNodeId = node.id;
  return { node, connector };
}

export function desktopDirectorySelectionAvailable(): boolean {
  return bridge() !== null;
}

/** A native user selection grants Git access independently of the browser target. */
export async function selectDesktopGitDirectory(): Promise<string | null> {
  if (!("zero" in window) || !object(window.zero)) {
    return null;
  }
  const owner = window.zero;
  const { invoke } = owner;
  if (typeof invoke !== "function") {
    return null;
  }
  const selected: unknown = await Reflect.apply(invoke, owner, [
    "swarmie.desktop.git.selectDirectory",
  ]);
  if (selected === null || typeof selected === "string") {
    return selected;
  }
  throw new Error("The desktop directory picker returned an invalid path.");
}
