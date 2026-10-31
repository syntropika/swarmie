import { useSource } from "@swarmie/client-ui";
import { Button, NavigationGroup } from "@swarmie/ui-web";
import { useState } from "react";

import { metadataTime } from "./freshness";
import type { ContextPresentation } from "./presentation";
import type { ContextView, Inspection } from "./types";

const labels = {
  instructions: "Instructions",
  conversation: "Conversation",
  tools: "Tools, including MCP",
  other: "Other contributions",
};
const phases = {
  planned: "Compaction queued",
  dispatched: "Compacting context",
  unknown: "Compaction outcome is uncertain",
  candidate: "Checking compacted context",
  activated: "Context compacted",
  superseded: "Compaction superseded",
  stopped: "Compaction stop recorded",
  rejected: "The compacted context could not be used",
  failed: "Compaction could not complete",
};
const compactPhases = {
  planned: "Queued",
  dispatched: "Compacting",
  unknown: "Uncertain",
  candidate: "Checking",
  activated: "Compacted",
  superseded: "Superseded",
  stopped: "Stop recorded",
  rejected: "Unusable result",
  failed: "Failed",
};
const number = new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 });

function title(inspection: Inspection | null | undefined) {
  if (!inspection || inspection.budget.input_limit === 0) {
    return "Context";
  }
  const { budget } = inspection;
  const estimate = budget.precision === "estimated" ? "≈ " : "";
  const occupied = number.format((100 * budget.used) / budget.input_limit);
  return `Context · ${estimate}${occupied}%${inspection.stale ? " · Previous" : ""}`;
}

function Measurement({ inspection }: { inspection: Inspection }) {
  const { budget } = inspection;
  const estimate = budget.precision === "estimated" ? "≈ " : "";
  return (
    <div className="grid gap-3">
      <div>
        <p className="font-medium tabular-nums">
          {estimate}
          {number.format(budget.used)} / {number.format(budget.input_limit)} tokens
        </p>
        <p className="text-xs text-muted">
          {inspection.stale ? "Previous preparation · Out of date" : "Last prepared request"} ·{" "}
          {budget.precision === "estimated" ? "Estimated" : "Exact"}
        </p>
      </div>
      <dl className="grid grid-cols-[1fr_auto] gap-x-4 gap-y-1 text-xs tabular-nums">
        {inspection.components.map((part) => (
          <div key={part.kind} className="contents">
            <dt>{labels[part.kind]}</dt>
            <dd>
              {estimate}
              {number.format(part.tokens)}
            </dd>
          </div>
        ))}
        <dt className="text-muted">Output reserved</dt>
        <dd>{number.format(budget.output_reserve)}</dd>
        <dt className="text-muted">Output bound</dt>
        <dd>{inspection.output_reservation === "sent_cap" ? "Request cap" : "Provider maximum"}</dd>
        <dt className="text-muted">Safety headroom</dt>
        <dd>{number.format(budget.headroom)}</dd>
        <dt className="text-muted">Available input</dt>
        <dd>
          {estimate}
          {number.format(budget.available)}
        </dd>
      </dl>
      {inspection.decision === "input_too_large" ? (
        <output className="text-xs">
          The complete new input and current contributions exceed this model’s budget. Choose a
          model with more room or edit the input.
        </output>
      ) : null}
      <p className="text-xs text-muted">
        {inspection.provider} · {inspection.model}
        <br />
        {number.format(inspection.bytes)} request bytes · {inspection.method}
      </p>
      <dl className="grid gap-1 text-xs text-muted">
        <div>
          <dt>Metadata prepared</dt>
          <dd>{metadataTime(inspection.metadata_freshness.prepared_at_ms)}</dd>
        </div>
        <div>
          <dt>Snapshot reuse boundary</dt>
          <dd>
            {inspection.metadata_freshness.valid_until_ms === null
              ? "No finite snapshot expiry"
              : metadataTime(inspection.metadata_freshness.valid_until_ms)}
          </dd>
        </div>
        <div>
          <dt>Stale limit evidence admitted</dt>
          <dd>{number.format(inspection.metadata_freshness.stale_limit_evidence)}</dd>
        </div>
      </dl>
      <p className="text-xs text-muted">
        Snapshot timing does not report live verification of every fact.
      </p>
    </div>
  );
}

function operationStatus(view: ContextView) {
  if (view.stopRequested && (view.phase === "unknown" || view.phase === "dispatched")) {
    return "Stop requested. Waiting for the operation to settle and its outcome to be recognized.";
  }
  return view.phase === null ? null : phases[view.phase];
}

/** Presentation reads the owner projection; no budget or execution policy lives here. */
export function ContextPanel({ api }: { api: ContextPresentation }) {
  const state = useSource(api.state);
  const [expanded, setExpanded] = useState(true);
  const checking = api.hasPending();
  const status = state.view === null ? null : operationStatus(state.view);
  const unavailable = state.view?.unavailable ?? null;
  return (
    <NavigationGroup
      title={title(state.view?.inspection)}
      expanded={expanded}
      onExpandedChange={setExpanded}
      actions={
        state.view?.phase ? (
          <span className="max-w-28 truncate pr-3 text-xs text-muted" title={status ?? undefined}>
            {compactPhases[state.view.phase]}
          </span>
        ) : null
      }
    >
      <div className="grid gap-3 px-3 pt-2 pb-3">
        {state.phase === "loading" ? (
          <output className="text-xs text-muted">Loading context…</output>
        ) : null}
        {state.view?.inspection ? (
          <Measurement inspection={state.view.inspection} />
        ) : (
          <p className="text-xs text-muted">
            {state.phase === "unavailable"
              ? "Connect a conversation node with Context support."
              : "Context has not been measured for a model request yet."}
          </p>
        )}
        {status === null ? null : <output className="text-xs">{status}</output>}
        {unavailable === null ? null : <p className="text-xs text-muted">{unavailable}</p>}
        {state.error === null ? null : <output className="text-xs">{state.error}</output>}
        <Button
          size="sm"
          variant="secondary"
          isDisabled={state.phase !== "ready" || (!checking && state.view?.canCompact !== true)}
          isPending={state.busy}
          onPress={() => {
            api.compact().catch(() => undefined);
          }}
        >
          {checking ? "Check compaction request" : "Compact context"}
        </Button>
        <p className="text-xs text-muted">Full conversation history is retained.</p>
      </div>
    </NavigationGroup>
  );
}
