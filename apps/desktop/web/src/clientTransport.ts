// Presentation code uses one client contract; native hosts provide its IPC adapter.
type NativeBridge = { invoke(command: string, payload: unknown): Promise<unknown> };
type TransferResponse = { response?: unknown; transferId?: string; chunks?: number };
function bridge(): NativeBridge | undefined {
  return typeof window === "undefined" ? undefined : (window as Window & { zero?: NativeBridge }).zero;
}
export function nativeClient(): boolean { return typeof bridge()?.invoke === "function" && new URLSearchParams(window.location.search).get("client") === "native"; }
async function invoke<T>(payload: unknown): Promise<T> {
  const native = bridge();
  if (!native) throw new Error("The native client bridge is unavailable.");
  return native.invoke("swarmie.client", payload) as Promise<T>;
}
export async function observeClient(): Promise<{ events: { environmentId?: string; method: string; params?: unknown }[] }> {
  return invoke({ operation: "observe" });
}
export async function clientRequest(path: string, request: unknown, token: string, timeout: number): Promise<Response> {
  if (!nativeClient()) return fetch(path, {
    method: "POST", headers: { "Content-Type": "application/json", ...(token ? { Authorization: `Bearer ${token}` } : {}) },
    body: JSON.stringify(request), signal: AbortSignal.timeout(timeout),
  });
  // Chunk transfers preserve the SDK's bounded message size for attachments and transcripts.
  const text = JSON.stringify(request);
  let staged: string | undefined;
  let result: TransferResponse;
  try {
    if (text.length <= 128 * 1024) result = await invoke({ operation: "rpc", path, request });
    else {
      for (let start = 0, index = 0; start < text.length; index++) {
        let end = Math.min(start + 128 * 1024, text.length);
        const last = text.charCodeAt(end - 1);
        if (end < text.length && last >= 0xd800 && last <= 0xdbff) end--;
        const reply = await invoke<{ transferId: string }>({ operation: "stage", transferId: staged, index, chunk: text.slice(start, end) });
        staged = reply.transferId; start = end;
      }
      result = await invoke({ operation: "dispatch", transferId: staged, path });
    }
    if (result.transferId) {
      staged = result.transferId;
      if (!Number.isInteger(result.chunks) || result.chunks! < 1 || result.chunks! > 512) throw new Error("Invalid native response transfer.");
      const chunks: string[] = [];
      for (let index = 0; index < result.chunks!; index++) chunks.push((await invoke<{ chunk: string }>({ operation: "read", transferId: staged, index })).chunk);
      return new Response(chunks.join(""), { headers: { "Content-Type": "application/json" } });
    }
    return new Response(JSON.stringify(result.response), { headers: { "Content-Type": "application/json" } });
  } finally {
    if (staged) await invoke({ operation: "release", transferId: staged }).catch(() => {});
  }
}
