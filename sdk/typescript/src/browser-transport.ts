import type { ClientOptions, Transport } from "./options.js";
import { ProtocolError } from "./protocol-error.js";
import { decodeResponse, positive } from "./wire.js";

export class HttpTransport implements Transport {
  private sequence = 0;
  private readonly prefix = crypto.randomUUID();
  private readonly endpoint: URL;
  private readonly options: ClientOptions;
  constructor(url: string, options: ClientOptions = {}) {
    this.options = options;
    this.endpoint = new URL(url);
    if (!["http:", "https:"].includes(this.endpoint.protocol)) {
      throw new ProtocolError("Use an HTTP or HTTPS endpoint");
    }
    if (this.endpoint.pathname === "/") {
      this.endpoint.pathname = "/rpc";
    }
    positive(options.timeoutMs ?? 30_000, "timeoutMs");
    positive(options.maxResponseBytes ?? 16 * 1024 * 1024, "maxResponseBytes");
  }
  async request(method: string, params: unknown, signal?: AbortSignal): Promise<unknown> {
    this.sequence += 1;
    const id = `${this.prefix}-${this.sequence}`;
    const timeout = AbortSignal.timeout(this.options.timeoutMs ?? 30_000);
    const cancellation = signal ? AbortSignal.any([signal, timeout]) : timeout;
    const response = await fetch(this.endpoint, {
      method: "POST",
      redirect: "error",
      headers: {
        "Content-Type": "application/json",
        ...(this.options.token !== undefined && this.options.token !== ""
          ? { Authorization: `Bearer ${this.options.token}` }
          : {}),
      },
      body: JSON.stringify({ jsonrpc: "2.0", id, method, params }),
      signal: cancellation,
    });
    if (!response.ok) {
      throw new ProtocolError(`Runtime HTTP status ${response.status}`);
    }
    const reader = response.body?.getReader();
    if (!reader) {
      throw new ProtocolError("Runtime response has no body");
    }
    const chunks: Uint8Array[] = [];
    let length = 0;
    const limit = this.options.maxResponseBytes ?? 16 * 1024 * 1024;
    for (;;) {
      // oxlint-disable-next-line no-await-in-loop -- A bounded stream reader must consume chunks sequentially.
      const { done, value } = await reader.read();
      if (done) {
        break;
      }
      length += value.length;
      if (length > limit) {
        // oxlint-disable-next-line no-await-in-loop -- A bounded stream reader must consume chunks sequentially.
        await reader.cancel();
        throw new ProtocolError("Runtime response exceeded the configured byte limit");
      }
      chunks.push(value);
    }
    const bytes = new Uint8Array(length);
    let offset = 0;
    for (const chunk of chunks) {
      bytes.set(chunk, offset);
      offset += chunk.length;
    }
    let value: unknown;
    try {
      value = JSON.parse(new TextDecoder().decode(bytes));
    } catch {
      throw new ProtocolError("Runtime response is not JSON");
    }
    return decodeResponse(value, id);
  }
}
