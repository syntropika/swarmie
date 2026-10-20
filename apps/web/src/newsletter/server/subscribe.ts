/** Newsletter interest is recorded through a server-side contact provider. */
export type NewsletterSubscriptions = {
  /** Record interest without sending email or explicitly changing subscription status. */
  subscribe: (email: string) => Promise<void>;
};

/** Public responses never disclose whether an address already exists. */
export type SubscriptionResponse = { ok: true } | { ok: false; error: string };

const MAX_BODY_BYTES = 4096;

const UNAVAILABLE_MESSAGE = "We could not save your email right now. Please try again later.";

/** Validate an untrusted request before passing its normalized email to the subscription port. */
export async function handleSubscription(
  request: Request,
  subscriptions?: NewsletterSubscriptions,
): Promise<Response> {
  if (request.method !== "POST") {
    return failure(405, "Please submit the form using POST.");
  }

  if (isCrossOrigin(request)) {
    return failure(403, "Please submit the form from this website.");
  }

  if (
    request.headers.get("content-type")?.split(";", 1)[0]?.trim().toLowerCase() !==
    "application/json"
  ) {
    return failure(415, "Please submit the form as JSON.");
  }

  const declaredLength = request.headers.get("content-length");
  if (declaredLength !== null && Number(declaredLength) > MAX_BODY_BYTES) {
    return failure(413, "The submitted form is too large.");
  }

  let payload: unknown;
  try {
    const text = await readBoundedBody(request);
    if (text === undefined) {
      return failure(413, "The submitted form is too large.");
    }
    payload = JSON.parse(text);
  } catch {
    return failure(400, "Please enter a valid email address.");
  }

  if (
    !isRecord(payload) ||
    (payload.website !== undefined && typeof payload.website !== "string")
  ) {
    return failure(400, "Please enter a valid email address.");
  }

  if (typeof payload.website === "string" && payload.website.trim() !== "") {
    return success();
  }

  if (typeof payload.email !== "string") {
    return failure(400, "Please enter a valid email address.");
  }
  const email = payload.email.trim().toLowerCase();
  if (!isEmail(email)) {
    return failure(400, "Please enter a valid email address.");
  }
  if (!subscriptions) {
    return failure(503, UNAVAILABLE_MESSAGE);
  }

  try {
    await subscriptions.subscribe(email);
    return success();
  } catch {
    return failure(503, UNAVAILABLE_MESSAGE);
  }
}

/** Read at most the form limit, including requests without a Content-Length header. */
async function readBoundedBody(request: Request): Promise<string | undefined> {
  if (!request.body) {
    return "";
  }
  const reader = request.body.getReader();
  const decoder = new TextDecoder();
  let bytes = 0;
  let text = "";

  try {
    while (true) {
      // Each read must wait for the previous chunk so the byte limit can stop the stream immediately.
      // oxlint-disable-next-line no-await-in-loop -- Stream consumption is ordered and bounded; parallel reads would over-consume input.
      const chunk = await reader.read();
      if (chunk.done) {
        return text + decoder.decode();
      }
      const value: unknown = chunk.value;
      if (!(value instanceof Uint8Array)) {
        throw new Error("Expected a byte stream");
      }
      bytes += value.byteLength;
      if (bytes > MAX_BODY_BYTES) {
        // oxlint-disable-next-line no-await-in-loop -- Cancellation must finish before this bounded reader releases its lock.
        await reader.cancel();
        return undefined;
      }
      text += decoder.decode(value, { stream: true });
    }
  } finally {
    reader.releaseLock();
  }
}

/** Narrow JSON objects without trusting provider or form payload types. */
export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** Accept ordinary internet mailboxes within the address and local-part length limits. */
function isEmail(email: string): boolean {
  if (email.length > 254) {
    return false;
  }
  const [local] = email.split("@", 1);
  if (
    local === undefined ||
    local.length === 0 ||
    local.length > 64 ||
    local.startsWith(".") ||
    local.endsWith(".") ||
    local.includes("..")
  ) {
    return false;
  }
  return /^[a-z0-9.!#$%&'*+/=?^_`{|}~-]+@[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?)+$/iu.test(
    email,
  );
}

/** Return uncached success for a recorded request, existing contact, or honeypot. */
function success(): Response {
  return json({ ok: true }, 200);
}

/** Expose only a local, actionable error message. */
function failure(status: number, error: string): Response {
  return json({ ok: false, error }, status);
}

/** Keep the response contract and cache policy consistent across all outcomes. */
function json(body: SubscriptionResponse, status: number): Response {
  return Response.json(body, { status, headers: { "Cache-Control": "no-store" } });
}

/** Browser origin evidence must agree with this route before any request data is consumed. */
function isCrossOrigin(request: Request): boolean {
  const origin = request.headers.get("origin");
  return (
    request.headers.get("sec-fetch-site") === "cross-site" ||
    (origin !== null && origin !== new URL(request.url).origin)
  );
}
