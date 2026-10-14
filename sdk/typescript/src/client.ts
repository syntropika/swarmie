import type { RuntimeEvent } from "@swarmie/contracts";

import { Client as BrowserClient } from "./browser.js";
import { HttpTransport } from "./http-transport.js";
import type { NativeTransport } from "./native-transport.js";
import { control, execute, handle, jsonResult } from "./native.js";
import type { ClientOptions, EventOptions } from "./options.js";
import { fromWire } from "./wire.js";

export type * from "@swarmie/contracts";
export { RpcError } from "./rpc-error.js";
export { ProtocolError } from "./protocol-error.js";
export { NativeTransport } from "./native-transport.js";
export { HttpTransport } from "./http-transport.js";
export type { ClientOptions, EventOptions } from "./options.js";

export function decodeResponse(value: unknown, id: string): unknown {
  return jsonResult(
    control({ action: "decode", response_json: JSON.stringify(value), request_id: id }),
  );
}
// Convenience methods only marshal the canonical wire types. Native events replace
// the browser polling implementation so Node shares all transport invariants with Rust.
export class Client extends BrowserClient {
  constructor(public override readonly transport: NativeTransport) {
    super(transport);
  }
  static override http(url: string, options: ClientOptions = {}): Client {
    return new Client(new HttpTransport(url, options));
  }
  dispose(): void {
    this.transport.dispose();
  }
  override async *events(
    sessionId: string,
    options: EventOptions = {},
  ): AsyncGenerator<RuntimeEvent> {
    const stream = handle({
      action: "open_events",
      client: this.transport.id,
      session_id: sessionId,
      options: {
        after_sequence: options.afterSequence ?? 0,
        timeout_ms: options.timeoutMs ?? 600_000,
        poll_interval_ms: options.pollIntervalMs ?? 200,
      },
    });
    try {
      for (;;) {
        // oxlint-disable-next-line no-await-in-loop -- Each pull consumes the next event of this cursor.
        const value = jsonResult(await execute({ action: "next_event", stream }, options.signal));
        // Rust validates session identity and sequence; schema generation owns event fields.
        yield fromWire<RuntimeEvent>(value);
      }
    } finally {
      control({ action: "release", handle: stream });
    }
  }
  override async waitForTurn(
    sessionId: string,
    options: EventOptions = {},
  ): Promise<RuntimeEvent[]> {
    const value = jsonResult(
      await execute(
        {
          action: "wait_turn",
          client: this.transport.id,
          session_id: sessionId,
          options: {
            after_sequence: options.afterSequence ?? 0,
            timeout_ms: options.timeoutMs ?? 600_000,
            poll_interval_ms: options.pollIntervalMs ?? 200,
          },
        },
        options.signal,
      ),
    );
    // Rust returns an array of events validated against the session and cursor.
    return fromWire<RuntimeEvent[]>(value);
  }
}
