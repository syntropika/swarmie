export type ClientOptions = { token?: string; timeoutMs?: number; maxResponseBytes?: number };
export type EventOptions = {
  afterSequence?: number;
  timeoutMs?: number;
  pollIntervalMs?: number;
  signal?: AbortSignal;
};
export type Transport = {
  request: (method: string, params: unknown, signal?: AbortSignal) => Promise<unknown>;
};
