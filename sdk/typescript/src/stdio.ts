import { Client } from "./client.js";
import { NativeTransport } from "./native-transport.js";
import { handle } from "./native.js";

export class StdioTransport extends NativeTransport {
  constructor(command: string, args: string[] = [], timeoutMs = 30_000) {
    super(
      handle({
        action: "create_stdio",
        config: { command, args, timeout_ms: timeoutMs, max_response_bytes: 16 * 1024 * 1024 },
      }),
    );
  }
  close(): void {
    this.dispose();
  }
}
export function stdioClient(command: string, args: string[] = [], timeoutMs = 30_000) {
  const transport = new StdioTransport(command, args, timeoutMs);
  return {
    client: new Client(transport),
    close: () => {
      transport.close();
    },
  };
}
