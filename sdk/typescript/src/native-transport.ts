import { execute, jsonResult, own, release } from "./native.js";

export class NativeTransport {
  constructor(public readonly id: number) {
    own(this, id);
  }
  async request(method: string, params: unknown, signal?: AbortSignal): Promise<unknown> {
    return jsonResult(
      await execute(
        {
          action: "invoke",
          client: this.id,
          request: { method, params_json: JSON.stringify(params), timeout_ms: null },
        },
        signal,
      ),
    );
  }
  dispose(): void {
    release(this, this.id);
  }
}
