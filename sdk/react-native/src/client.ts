import type { Methods, RuntimeEvent } from "@swarmie/contracts";
import { decodeContract, decodeRuntimeResult } from "@swarmie/contracts/validation";

import { NativeClientError } from "./error";

/** Thin lowering of sdk/wit/client.wit; Rust owns networking, deadlines and cursors. */
export type NativeBinding = {
  control: (command: string) => string;
  execute: (command: string) => Promise<string>;
};

export { NativeClientError } from "./error";

function unwrap(encoded: string): unknown {
  const value: unknown = JSON.parse(encoded);
  if (typeof value !== "object" || value === null) {
    throw new NativeClientError("protocol");
  }
  if ("ok" in value && !("error" in value)) {
    return value.ok;
  }
  if ("error" in value && typeof value.error === "object" && value.error !== null) {
    const { error } = value;
    if ("kind" in error && typeof error.kind === "string") {
      // Native messages may contain remote paths or provider output. Keep UI errors bounded.
      throw new NativeClientError(error.kind);
    }
  }
  throw new NativeClientError("protocol");
}

function jsonResult(value: unknown): unknown {
  if (typeof value !== "string") {
    throw new NativeClientError("protocol");
  }
  return JSON.parse(value);
}

export class MobileClient {
  private readonly id: number;
  private closed = false;

  constructor(
    private readonly binding: NativeBinding,
    config: { url: string; token: string },
  ) {
    this.id = this.handle({
      action: "create_http",
      config: {
        url: config.url,
        token: config.token,
        timeout_ms: 30_000,
        max_response_bytes: 16 * 1024 * 1024,
      },
    });
  }

  private control(command: object): unknown {
    return unwrap(this.binding.control(JSON.stringify(command)));
  }

  private handle(command: object): number {
    const value = this.control(command);
    if (typeof value !== "number" || !Number.isSafeInteger(value) || value <= 0) {
      throw new NativeClientError("protocol");
    }
    return value;
  }

  private async execute(command: object, signal?: AbortSignal): Promise<unknown> {
    if (this.closed) {
      throw new NativeClientError("closed");
    }
    const operation = this.handle({ action: "prepare" });
    const cancel = () => {
      this.control({ action: "cancel", operation });
    };
    signal?.addEventListener("abort", cancel, { once: true });
    try {
      if (signal?.aborted === true) {
        cancel();
      }
      return unwrap(await this.binding.execute(JSON.stringify({ ...command, operation })));
    } finally {
      signal?.removeEventListener("abort", cancel);
      this.control({ action: "release", handle: operation });
    }
  }

  async call<M extends keyof Methods>(
    method: M,
    params: Methods[M]["params"],
    signal?: AbortSignal,
  ): Promise<Methods[M]["result"]> {
    const result = await this.execute(
      {
        action: "invoke",
        client: this.id,
        request: { method, params_json: JSON.stringify(params), timeout_ms: null },
      },
      signal,
    );
    return decodeRuntimeResult(method, jsonResult(result));
  }

  async *events(
    sessionId: string,
    afterSequence: number,
    signal: AbortSignal,
  ): AsyncGenerator<RuntimeEvent> {
    const stream = this.handle({
      action: "open_events",
      client: this.id,
      session_id: sessionId,
      options: { after_sequence: afterSequence, timeout_ms: 600_000, poll_interval_ms: 250 },
    });
    try {
      while (!signal.aborted) {
        // oxlint-disable-next-line no-await-in-loop -- Pulls consume the Rust-owned cursor in order.
        const result = await this.execute({ action: "next_event", stream }, signal);
        yield decodeContract("RuntimeEvent", jsonResult(result));
      }
    } finally {
      this.control({ action: "release", handle: stream });
    }
  }

  dispose(): void {
    if (!this.closed) {
      this.closed = true;
      this.control({ action: "release", handle: this.id });
    }
  }
}
