import { RemoteHost } from "@syntropika/tembra-sdk/rpc";
import type { Transport } from "@syntropika/tembra-sdk/rpc";

import { NodeConnectionError, connectionFailure } from "./errors";
import { ConnectionFailureKind, NodeProtocol } from "./types";
import type { Connector, NodeIdentity } from "./types";

function identityValue(value: unknown): NodeIdentity {
  if (
    typeof value !== "object" ||
    value === null ||
    !("nodeId" in value) ||
    typeof value.nodeId !== "string" ||
    value.nodeId === "" ||
    !("principal" in value) ||
    typeof value.principal !== "string" ||
    value.principal === "" ||
    !("protocolVersion" in value) ||
    !Number.isSafeInteger(value.protocolVersion)
  ) {
    throw new NodeConnectionError(
      ConnectionFailureKind.Protocol,
      "The node returned an invalid identity.",
    );
  }
  // The integer check above excludes non-numbers without accepting a string conversion.
  if (typeof value.protocolVersion !== "number") {
    throw new NodeConnectionError(
      ConnectionFailureKind.Protocol,
      "The node version must be numeric.",
    );
  }
  return {
    nodeId: value.nodeId,
    principal: value.principal,
    protocolVersion: value.protocolVersion,
  };
}

async function opened(socket: WebSocket, signal: AbortSignal) {
  // oxlint-disable-next-line promise/avoid-new -- WebSocket exposes opening/close events rather than an opening promise.
  await new Promise<void>((resolve, reject) => {
    function clear() {
      socket.removeEventListener("open", ready);
      socket.removeEventListener("error", failed);
      socket.removeEventListener("close", failed);
      signal.removeEventListener("abort", aborted);
    }
    function ready() {
      clear();
      resolve();
    }
    function failed() {
      clear();
      reject(
        new NodeConnectionError(ConnectionFailureKind.Network, "Unable to connect to the node."),
      );
    }
    function aborted() {
      clear();
      reject(new NodeConnectionError(ConnectionFailureKind.Network, "Connection cancelled."));
    }
    socket.addEventListener("open", ready, { once: true });
    socket.addEventListener("error", failed, { once: true });
    socket.addEventListener("close", failed, { once: true });
    signal.addEventListener("abort", aborted, { once: true });
    if (signal.aborted) {
      aborted();
    }
  });
}

/** Direct byte adapter. The endpoint's authentication belongs to the embedding. */
export const connectWebSocket: Connector = async (node, signal, lost) => {
  signal.throwIfAborted();
  if (node.url === null) {
    throw new NodeConnectionError(
      ConnectionFailureKind.Configuration,
      "Configure a node connection first.",
    );
  }
  let url: URL;
  try {
    url = new URL(node.url);
  } catch {
    throw new NodeConnectionError(ConnectionFailureKind.Configuration, "Provide a valid node URL.");
  }
  if (url.protocol !== "ws:" && url.protocol !== "wss:") {
    throw new NodeConnectionError(ConnectionFailureKind.Configuration, "Use a WebSocket node URL.");
  }
  const socket = new WebSocket(url);
  socket.binaryType = "arraybuffer";
  let intentional = false;
  let notified = false;
  let receiveFailure: Error | undefined;
  let receiving = false;
  let remote: RemoteHost | null = null;
  function close() {
    intentional = true;
    signal.removeEventListener("abort", close);
    remote?.close();
    socket.close();
  }
  signal.addEventListener("abort", close, { once: true });
  try {
    await opened(socket, signal);
    const transport: Transport = {
      send(frame) {
        socket.send(new Uint8Array(frame));
      },
      listen(receive, closed) {
        function message(event: MessageEvent<unknown>) {
          receiveFailure = undefined;
          receiving = true;
          try {
            if (event.data instanceof ArrayBuffer) {
              receive(new Uint8Array(event.data));
            } else {
              receiveFailure = new NodeConnectionError(
                ConnectionFailureKind.Protocol,
                "Expected a binary Tembra frame.",
              );
              closed(receiveFailure);
            }
          } finally {
            receiving = false;
            receiveFailure = undefined;
          }
        }
        function ended(event: CloseEvent) {
          if (event.code === 1002 || event.code === 1003 || event.code === 1008) {
            receiveFailure = new NodeConnectionError(
              ConnectionFailureKind.Protocol,
              "The node refused the channel protocol or policy.",
            );
          } else {
            receiveFailure =
              event.code === 1009
                ? new NodeConnectionError(
                    ConnectionFailureKind.Resource,
                    "The node connection exceeded its resource limit.",
                  )
                : new NodeConnectionError(ConnectionFailureKind.Network, "Node connection ended.");
          }
          closed(receiveFailure);
          receiveFailure = undefined;
        }
        socket.addEventListener("message", message);
        socket.addEventListener("close", ended);
        return () => {
          socket.removeEventListener("message", message);
          socket.removeEventListener("close", ended);
        };
      },
      close(cause?: unknown) {
        signal.removeEventListener("abort", close);
        socket.close();
        if (!intentional && !notified) {
          notified = true;
          lost(
            (receiving ? receiveFailure : undefined) ??
              cause ??
              receiveFailure ??
              new NodeConnectionError(ConnectionFailureKind.Network, "Node connection ended."),
          );
        }
      },
    };
    remote = new RemoteHost(transport, {
      maxFrame: NodeProtocol.MaxFrame,
      maxPending: NodeProtocol.MaxPending,
      // SDK 0.1 reports receive failures before synchronous channel closure. Only
      // an actual transport.close consumes this cause; observer errors do not end a session.
      onError(error: unknown) {
        console.error(error);
        if (!receiving) {
          return;
        }
        const failure = connectionFailure(error);
        receiveFailure = new NodeConnectionError(
          failure.kind === ConnectionFailureKind.Network
            ? ConnectionFailureKind.Protocol
            : failure.kind,
          failure.message,
        );
      },
    });
    const peer = remote;
    const reference = await peer.get(NodeProtocol.Service, NodeProtocol.Key, { signal });
    let identity: NodeIdentity;
    try {
      const identify: unknown = Reflect.get(reference, "identify");
      if (typeof identify !== "function") {
        throw new NodeConnectionError(
          ConnectionFailureKind.Protocol,
          "The node identity service is unavailable.",
        );
      }
      identity = identityValue(await Reflect.apply(identify, undefined, []));
    } finally {
      reference.release();
    }
    signal.throwIfAborted();
    return {
      identity,
      get: async (service, key, options) => await peer.get(service, key, options),
      close,
    };
  } catch (error: unknown) {
    close();
    throw error;
  }
};
