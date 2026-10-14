import type {
  Methods,
  RuntimeEvent,
  SessionSnapshot,
  InputSpawnOptions,
  InputPromptContent,
} from "@swarmie/contracts";

import { HttpTransport } from "./browser-transport.js";
import type { ClientOptions, EventOptions, Transport } from "./options.js";
import { ProtocolError } from "./protocol-error.js";
import { fromWire, positive } from "./wire.js";

export type * from "@swarmie/contracts";
export { RpcError } from "./rpc-error.js";
export { ProtocolError } from "./protocol-error.js";
export { decodeResponse } from "./wire.js";
export { HttpTransport } from "./browser-transport.js";
export type { ClientOptions, EventOptions, Transport } from "./options.js";

export class Client {
  constructor(public readonly transport: Transport) {}
  static http(url: string, options: ClientOptions = {}) {
    return new Client(new HttpTransport(url, options));
  }
  async call<M extends keyof Methods>(
    method: M,
    params: Methods[M]["params"],
    signal?: AbortSignal,
  ): Promise<Methods[M]["result"]> {
    return fromWire<Methods[M]["result"]>(await this.transport.request(method, params, signal));
  }
  async callRaw(method: string, params: unknown = {}, signal?: AbortSignal): Promise<unknown> {
    return await this.transport.request(method, params, signal);
  }
  async initialize() {
    return await this.call("runtime/initialize", {});
  }
  async schema() {
    return await this.call("runtime/schema", {});
  }
  async discover() {
    return await this.call("runtime/discover", {});
  }
  async spawn(options: InputSpawnOptions): Promise<SessionSnapshot> {
    return await this.call("session/create", options);
  }
  async prompt(sessionId: string, prompt: string | InputPromptContent[], idempotencyKey: string) {
    return await this.call("session/prompt", {
      sessionId,
      idempotencyKey,
      ...(typeof prompt === "string" ? { text: prompt } : { content: prompt }),
    });
  }
  async submission(sessionId: string, idempotencyKey: string) {
    return await this.call("session/submission", { sessionId, idempotencyKey });
  }
  async reconcileSubmission(sessionId: string, idempotencyKey: string) {
    return await this.call("session/submission/reconcile", { sessionId, idempotencyKey });
  }
  async close(sessionId: string) {
    return await this.call("session/close", { sessionId });
  }
  async *events(sessionId: string, options: EventOptions = {}): AsyncGenerator<RuntimeEvent> {
    let sequence = options.afterSequence ?? 0;
    const timeout = positive(options.timeoutMs ?? 600_000, "timeoutMs");
    const interval = positive(options.pollIntervalMs ?? 200, "pollIntervalMs");
    const end = performance.now() + timeout;
    while (performance.now() < end) {
      options.signal?.throwIfAborted();
      const signal = AbortSignal.any([
        AbortSignal.timeout(Math.max(1, Math.ceil(end - performance.now()))),
        ...(options.signal ? [options.signal] : []),
      ]);
      // oxlint-disable-next-line no-await-in-loop -- Transcript pages depend on the previous sequence.
      const events = await this.call(
        "session/transcript",
        { sessionId, afterSequence: sequence, limit: 256 },
        signal,
      );
      for (const event of events) {
        if (event.sequence <= sequence || event.sessionId !== sessionId) {
          throw new ProtocolError("Runtime event ordering or session identity mismatch");
        }
        ({ sequence } = event);
        yield event;
      }
      if (events.length < 256) {
        // oxlint-disable-next-line no-await-in-loop, promise/avoid-new -- Polling waits are sequential and browser timers require an abortable promise.
        await new Promise<void>((resolve, reject) => {
          const timer = setTimeout(done, Math.min(interval, end - performance.now()));
          function done() {
            options.signal?.removeEventListener("abort", cancel);
            resolve();
          }
          function cancel() {
            clearTimeout(timer);
            options.signal?.removeEventListener("abort", cancel);
            reject(new DOMException("Event polling cancelled", "AbortError"));
          }
          options.signal?.addEventListener("abort", cancel, { once: true });
        });
      }
    }
    throw new ProtocolError("Event polling deadline exceeded");
  }
  async waitForTurn(sessionId: string, options: EventOptions = {}): Promise<RuntimeEvent[]> {
    const events: RuntimeEvent[] = [];
    for await (const event of this.events(sessionId, options)) {
      events.push(event);
      if (event.type === "turn.ended") {
        return events;
      }
      if (event.type === "session.ended") {
        throw new ProtocolError("Session ended before the turn completed");
      }
    }
    return events;
  }
}
