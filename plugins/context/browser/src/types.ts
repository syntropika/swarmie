export const compactCommand = "swarmie.context.compact";

export type Budget = {
  used: number;
  context: number;
  input_limit: number;
  output_reserve: number;
  headroom: number;
  available: number;
  excess: number;
  precision: "exact" | "estimated";
};

export type MetadataFreshness = {
  prepared_at_ms: string;
  valid_until_ms: string | null;
  stale_limit_evidence: number;
};

export type Inspection = {
  model: string;
  provider: string;
  generation: string;
  method: string;
  bytes: number;
  stale: boolean;
  metadata_freshness: MetadataFreshness;
  output_reservation: "sent_cap" | "native_maximum";
  decision: "ready" | "compaction_required" | "input_too_large";
  budget: Budget;
  components: { kind: "instructions" | "conversation" | "tools" | "other"; tokens: number }[];
};

export type ContextView = {
  fence: string | null;
  inspection: Inspection | null;
  operation: string | null;
  phase:
    | "planned"
    | "dispatched"
    | "unknown"
    | "candidate"
    | "activated"
    | "superseded"
    | "stopped"
    | "rejected"
    | "failed"
    | null;
  stopRequested: boolean;
  canCompact: boolean;
  unavailable: string | null;
};

export type Selection = { nodeId: string; conversationId: string };
export type Pending = Selection & { principal: string; operation: string; fence: string };
export type Presentation = {
  selection: Selection | null;
  phase: "unavailable" | "loading" | "ready";
  view: ContextView | null;
  busy: boolean;
  error: string | null;
  pending: Pending[];
};

export const initialPresentation: Presentation = {
  selection: null,
  phase: "unavailable",
  view: null,
  busy: false,
  error: null,
  pending: [],
};
