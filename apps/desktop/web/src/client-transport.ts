// Presentation code uses one client contract; native hosts provide its IPC adapter.
type NativeBridge = { invoke: (command: string, payload: unknown) => Promise<unknown> };
type TransferResponse = { response?: unknown; transferId?: string; chunks?: number };
type ClientObservation = { events: { environmentId?: string; method: string; params?: unknown }[] };
function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
function nativeBridge(value: unknown): value is NativeBridge {
  return record(value) && typeof value.invoke === "function";
}
function bridge(): NativeBridge | undefined {
  if (typeof window === "undefined" || !("zero" in window)) {
    return undefined;
  }
  return nativeBridge(window.zero) ? window.zero : undefined;
}
export function nativeClient(): boolean {
  return (
    typeof bridge()?.invoke === "function" &&
    new URLSearchParams(window.location.search).get("client") === "native"
  );
}
async function invoke(payload: unknown): Promise<unknown> {
  const native = bridge();
  if (!native) {
    throw new Error("The native client bridge is unavailable.");
  }
  return await native.invoke("swarmie.client", payload);
}
export async function observeClient(): Promise<ClientObservation> {
  const value = await invoke({ operation: "observe" });
  if (!record(value) || !Array.isArray(value.events)) {
    throw new Error("Invalid native observation.");
  }
  const events = value.events.map((event: unknown) => {
    if (
      !record(event) ||
      typeof event.method !== "string" ||
      (event.environmentId !== undefined && typeof event.environmentId !== "string")
    ) {
      throw new Error("Invalid native observation event.");
    }
    return {
      method: event.method,
      ...(typeof event.environmentId === "string" ? { environmentId: event.environmentId } : {}),
      ...(event.params === undefined ? {} : { params: event.params }),
    };
  });
  return { events };
}
function transfer(value: unknown): TransferResponse {
  if (
    !record(value) ||
    (value.transferId !== undefined && typeof value.transferId !== "string") ||
    (value.chunks !== undefined && typeof value.chunks !== "number")
  ) {
    throw new Error("Invalid native response transfer.");
  }
  return {
    response: value.response,
    ...(typeof value.transferId === "string" ? { transferId: value.transferId } : {}),
    ...(typeof value.chunks === "number" ? { chunks: value.chunks } : {}),
  };
}
async function readChunks(id: string, count: number, index = 0): Promise<string[]> {
  if (index === count) {
    return [];
  }
  const reply = await invoke({ operation: "read", transferId: id, index });
  if (!record(reply) || typeof reply.chunk !== "string") {
    throw new Error("Invalid native response chunk.");
  }
  return [reply.chunk, ...(await readChunks(id, count, index + 1))];
}
export async function clientRequest(
  path: string,
  request: unknown,
  token: string,
  timeout: number,
): Promise<Response> {
  if (!nativeClient()) {
    return await fetch(path, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        ...(token.length > 0 ? { Authorization: `Bearer ${token}` } : {}),
      },
      body: JSON.stringify(request),
      signal: AbortSignal.timeout(timeout),
    });
  }
  // Chunk transfers preserve bounded message size. Stage/read operations stay ordered.
  const text = JSON.stringify(request);
  if (text === undefined) {
    throw new Error("The native request must be JSON serializable.");
  }
  let staged: string | undefined;
  async function stage(start: number, index: number): Promise<string> {
    let end = Math.min(start + 128 * 1024, text.length);
    // Slicing strings uses UTF-16 offsets; checking the preceding code point would
    // consume a complete surrogate pair and miss an unfinished high surrogate.
    const trailingUnit = text.slice(end - 1, end).codePointAt(0) ?? 0;
    if (end < text.length && trailingUnit >= 0xd8_00 && trailingUnit <= 0xdb_ff) {
      end -= 1;
    }
    const reply = transfer(
      await invoke({
        operation: "stage",
        transferId: staged,
        index,
        chunk: text.slice(start, end),
      }),
    );
    if (reply.transferId === undefined || reply.transferId.length === 0) {
      throw new Error("Invalid native request transfer.");
    }
    staged = reply.transferId;
    return end < text.length ? await stage(end, index + 1) : staged;
  }

  try {
    const result = transfer(
      text.length <= 128 * 1024
        ? await invoke({ operation: "rpc", path, request })
        : await invoke({ operation: "dispatch", transferId: await stage(0, 0), path }),
    );
    if (result.transferId !== undefined && result.transferId.length > 0) {
      staged = result.transferId;
      const { chunks } = result;
      if (chunks === undefined || !Number.isInteger(chunks) || chunks < 1 || chunks > 512) {
        throw new Error("Invalid native response transfer.");
      }
      const responseChunks = await readChunks(staged, chunks);
      return new Response(responseChunks.join(""), {
        headers: { "Content-Type": "application/json" },
      });
    }
    return Response.json(result.response);
  } finally {
    if (staged !== undefined) {
      // Release is best effort after a settled request; native teardown owns expired transfers.
      try {
        await invoke({ operation: "release", transferId: staged });
      } catch {
        staged = undefined;
      }
    }
  }
}
