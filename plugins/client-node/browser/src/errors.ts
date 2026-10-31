import { errorMessage } from "@swarmie/client-ui";
import { Code, TembraError } from "@syntropika/tembra-sdk";

import { ConnectionFailureKind } from "./types";
import type { ConnectionFailure, ConnectionFailureKindValue } from "./types";

export class NodeConnectionError extends Error {
  constructor(
    readonly kind: ConnectionFailureKindValue,
    message: string,
  ) {
    super(message);
    this.name = "NodeConnectionError";
  }
}

export function connectionFailure(error: unknown): ConnectionFailure {
  if (error instanceof NodeConnectionError) {
    return { kind: error.kind, message: error.message };
  }
  if (error instanceof SyntaxError) {
    return {
      kind: ConnectionFailureKind.Protocol,
      message: "The node sent invalid protocol data.",
    };
  }
  if (error instanceof TembraError) {
    if (error.code === Code.Exhausted) {
      return { kind: ConnectionFailureKind.Resource, message: error.message };
    }
    if (error.code === Code.Invalid) {
      return { kind: ConnectionFailureKind.Protocol, message: error.message };
    }
    if (error.code === Code.Forbidden) {
      return { kind: ConnectionFailureKind.Principal, message: error.message };
    }
  }
  return { kind: ConnectionFailureKind.Network, message: errorMessage(error) };
}
