import { NativeTransport } from "./native-transport.js";
import { handle } from "./native.js";
import type { ClientOptions } from "./options.js";

export class HttpTransport extends NativeTransport {
  constructor(url: string, options: ClientOptions = {}) {
    super(
      handle({
        action: "create_http",
        config: {
          url,
          token: options.token ?? null,
          timeout_ms: options.timeoutMs ?? 30_000,
          max_response_bytes: options.maxResponseBytes ?? 16 * 1024 * 1024,
        },
      }),
    );
  }
}
