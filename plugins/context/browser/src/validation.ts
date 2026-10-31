import { metadataFreshness } from "./freshness";
import type { Budget, ContextView, Inspection } from "./types";
import { compactCommand } from "./types";

function invalid(): never {
  throw new Error("The node returned an invalid Context projection.");
}
function isObject(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}
function object(value: unknown): Record<string, unknown> {
  if (!isObject(value)) {
    return invalid();
  }
  return value;
}
function text(value: unknown): string {
  if (typeof value !== "string" || value.length === 0 || value.length > 1024) {
    return invalid();
  }
  return value;
}
function count(value: unknown): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) {
    return invalid();
  }
  return value;
}
function boolean(value: unknown): boolean {
  if (typeof value !== "boolean") {
    return invalid();
  }
  return value;
}
function precision(value: unknown): "exact" | "estimated" {
  if (value !== "exact" && value !== "estimated") {
    return invalid();
  }
  return value;
}
function fence(value: unknown): string | null {
  if (value === null) {
    return null;
  }
  if (
    typeof value !== "string" ||
    !/^[\da-f]{8}-[\da-f]{4}-[\da-f]{4}-[\da-f]{4}-[\da-f]{12}$/iu.test(value) ||
    value === "00000000-0000-0000-0000-000000000000"
  ) {
    return invalid();
  }
  return value;
}
function budget(value: unknown): Budget {
  const raw = object(value);
  return {
    used: count(raw.used),
    context: count(raw.context),
    input_limit: count(raw.input_limit),
    output_reserve: count(raw.output_reserve),
    headroom: count(raw.headroom),
    available: count(raw.available),
    excess: count(raw.excess),
    precision: precision(raw.precision),
  };
}
function inspection(value: unknown): Inspection | null {
  if (value === null) {
    return null;
  }
  const raw = object(value);
  const prepared = object(raw.prepared);
  const model = object(prepared.model);
  const identity = object(model.identity);
  const measurement = object(prepared.measurement);
  const decision = object(raw.decision);
  if (
    decision.kind !== "ready" &&
    decision.kind !== "compaction_required" &&
    decision.kind !== "input_too_large"
  ) {
    return invalid();
  }
  if (!Array.isArray(measurement.components) || measurement.components.length > 4) {
    return invalid();
  }
  const components = measurement.components.map(
    (part: unknown): Inspection["components"][number] => {
      const item = object(part);
      if (
        item.kind !== "instructions" &&
        item.kind !== "conversation" &&
        item.kind !== "tools" &&
        item.kind !== "other"
      ) {
        return invalid();
      }
      return { kind: item.kind, tokens: count(item.tokens) };
    },
  );
  const result = budget(decision.budget);
  const reservation = object(prepared.output_reservation).kind;
  if (reservation !== "sent_cap" && reservation !== "native_maximum") {
    return invalid();
  }
  if (count(prepared.output_reserve) !== result.output_reserve) {
    return invalid();
  }
  if (reservation === "native_maximum") {
    const proof = object(model.native_output_maximum);
    const maximum = object(proof.value);
    if (
      proof.status !== "supported" ||
      maximum.accounting !== "total_generated_tokens" ||
      count(maximum.tokens) !== result.output_reserve
    ) {
      return invalid();
    }
  }
  if (
    new Set(components.map((part) => part.kind)).size !== components.length ||
    components.reduce((sum, part) => sum + part.tokens, 0) !== count(measurement.input_tokens) ||
    result.used !== measurement.input_tokens ||
    result.precision !== precision(measurement.precision)
  ) {
    return invalid();
  }
  return {
    model: text(identity.model),
    provider: text(identity.provider),
    generation: text(model.generation),
    method: text(measurement.method),
    bytes: count(measurement.final_payload_bytes),
    stale: boolean(raw.stale),
    metadata_freshness: metadataFreshness(model.metadata_freshness),
    output_reservation: reservation,
    decision: decision.kind,
    budget: result,
    components,
  };
}

/** Reduce bounded wire State to display data. This does not make admission decisions. */
export function contextView(value: unknown): ContextView {
  const raw = object(value);
  const checkpoint = object(raw.checkpoint);
  const admissionFence = fence(raw.fence);
  const canCompact = boolean(raw.can_compact);
  if (canCompact && admissionFence === null) {
    return invalid();
  }
  let operation: string | null = null;
  let phase: ContextView["phase"] = null;
  let stopRequested = false;
  if (checkpoint.compaction !== null) {
    const compaction = object(checkpoint.compaction);
    const status = object(compaction.phase);
    const { kind } = status;
    if (
      kind !== "planned" &&
      kind !== "dispatched" &&
      kind !== "unknown" &&
      kind !== "candidate" &&
      kind !== "activated" &&
      kind !== "superseded" &&
      kind !== "stopped" &&
      kind !== "rejected" &&
      kind !== "failed"
    ) {
      return invalid();
    }
    operation = text(compaction.operation);
    phase = kind;
    stopRequested = boolean(compaction.stop_requested);
  }
  return {
    fence: admissionFence,
    inspection: inspection(raw.inspection),
    operation,
    phase,
    stopRequested,
    canCompact,
    unavailable: raw.unavailable === null ? null : text(raw.unavailable),
  };
}

export function compactAlias(value: unknown): string | null {
  if (!Array.isArray(value)) {
    return invalid();
  }
  const entries = value.map(object).filter((entry) => entry.id === compactCommand);
  if (entries.length !== 1) {
    throw new Error("Context compaction is unavailable on this node.");
  }
  const [entry] = entries;
  if (entry === undefined || !Array.isArray(entry.aliases)) {
    return invalid();
  }
  const aliases: unknown[] = entry.aliases;
  const alias = aliases.find(
    (item: unknown) => typeof item === "string" && /^\/[a-z][a-z0-9-]*$/u.test(item),
  );
  return typeof alias === "string" ? alias.slice(1) : null;
}

export function receipt(value: unknown, requested: string) {
  const raw = object(value);
  if (raw.requested !== requested) {
    return invalid();
  }
  const operation = text(raw.operation);
  const admission = object(raw.admission);
  if (
    admission.kind !== "admitted" &&
    admission.kind !== "joined" &&
    admission.kind !== "recognized" &&
    admission.kind !== "unknown" &&
    admission.kind !== "refused"
  ) {
    return invalid();
  }
  return {
    operation,
    admission: admission.kind,
    view: contextView(raw.snapshot),
    reason: admission.kind === "refused" ? text(admission.reason) : null,
  };
}
