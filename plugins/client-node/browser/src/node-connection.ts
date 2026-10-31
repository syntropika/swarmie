import { NodeConnectionError } from "./errors";
import { ConnectionFailureKind } from "./types";
import type { NodeSession } from "./types";

/** Immutable provenance for one session; references never move to a replacement. */
export class NodeConnection {
  constructor(
    readonly nodeId: string,
    readonly principal: string,
    readonly generation: number,
    private readonly session: NodeSession,
    private readonly current: () => boolean,
  ) {}

  isCurrent() {
    return this.current();
  }

  async get(service: string, key: string | readonly string[], options?: { signal?: AbortSignal }) {
    this.check();
    const reference = await this.session.get(service, key, options);
    if (!this.isCurrent()) {
      reference.release();
      this.check();
    }
    return reference;
  }

  private check() {
    if (!this.isCurrent()) {
      throw new NodeConnectionError(ConnectionFailureKind.Network, "The node session has ended.");
    }
  }
}
