import type { DynamicReference } from "@syntropika/tembra-sdk";

type Revision = number | string;
type Cursor = { revision: Revision; offset: number };
type Page = { revision: Revision; data: string; next: Cursor | null };

function object(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}
function revision(value: unknown): value is Revision {
  return (
    (typeof value === "string" && value.length > 0) ||
    (typeof value === "number" && Number.isSafeInteger(value) && value >= 0)
  );
}
function page(value: unknown): Page {
  if (
    !object(value) ||
    !revision(value.revision) ||
    typeof value.data !== "string" ||
    value.data === "" ||
    !(
      value.next === null ||
      (object(value.next) &&
        revision(value.next.revision) &&
        typeof value.next.offset === "number" &&
        Number.isSafeInteger(value.next.offset) &&
        value.next.offset > 0)
    ) ||
    new TextEncoder().encode(JSON.stringify(value)).byteLength > 48 * 1024
  ) {
    throw new TypeError("The node returned an invalid read page.");
  }
  let next: Cursor | null = null;
  if (value.next !== null) {
    if (
      !object(value.next) ||
      !revision(value.next.revision) ||
      typeof value.next.offset !== "number"
    ) {
      throw new TypeError("The read cursor is invalid.");
    }
    next = { revision: value.next.revision, offset: value.next.offset };
  }
  return { revision: value.revision, data: value.data, next };
}
async function invoke(
  reference: DynamicReference,
  method: string,
  input?: Cursor,
): Promise<unknown> {
  const operation: unknown = Reflect.get(reference, method);
  if (typeof operation !== "function") {
    throw new TypeError("The node does not support paged reads.");
  }
  return await Reflect.apply(operation, undefined, input === undefined ? [] : [input]);
}

/** Assemble one lossless owner projection over existing RPC. Discard any retired session. */
export async function readPagedSnapshot(
  reference: DynamicReference,
  current: () => boolean,
  minimumRevision = 0,
  initialMethod = "list",
): Promise<unknown> {
  function check() {
    if (!current()) {
      throw new Error("Node access changed during the read.");
    }
  }
  check();
  const first = page(await invoke(reference, initialMethod));
  check();
  if (typeof first.revision === "number" && first.revision < minimumRevision) {
    throw new Error("The read snapshot is stale.");
  }
  const expected = first.revision;
  let data = "";
  let bytes = 0;
  let part = first;
  while (true) {
    check();
    if (part.revision !== expected) {
      throw new Error("Read pages belong to different revisions.");
    }
    data += part.data;
    bytes += new TextEncoder().encode(part.data).byteLength;
    if (part.next === null) {
      break;
    }
    if (part.next.revision !== expected || part.next.offset !== bytes) {
      throw new Error("The read cursor is stale or discontinuous.");
    }
    // oxlint-disable-next-line no-await-in-loop -- Each page requires the preceding owner cursor; parallel reads cannot preserve that sequence.
    part = page(await invoke(reference, "page", part.next));
    check();
  }
  check();
  const value: unknown = JSON.parse(data);
  if (object(value) && "revision" in value && value.revision !== expected) {
    throw new Error("The decoded snapshot revision differs from its pages.");
  }
  return value;
}
