import { expect, test } from "bun:test";

import { contextView } from "../src/validation";

function projection(kind: string, proof: unknown, reserve = 20) {
  return {
    checkpoint: { compaction: null },
    fence: null,
    can_compact: false,
    unavailable: null,
    inspection: {
      stale: false,
      prepared: {
        output_reserve: reserve,
        output_reservation: { kind },
        model: {
          identity: { provider: "fixture", model: "one" },
          generation: "a".repeat(64),
          native_output_maximum: proof,
          metadata_freshness: {
            prepared_at_ms: "0",
            valid_until_ms: null,
            stale_limit_evidence: 0,
          },
        },
        measurement: {
          input_tokens: 10,
          final_payload_bytes: 40,
          precision: "estimated",
          method: "fixture",
          components: [{ kind: "conversation", tokens: 10 }],
        },
      },
      decision: {
        kind: "ready",
        budget: {
          used: 10,
          context: 100,
          input_limit: 80,
          output_reserve: 20,
          headroom: 0,
          available: 70,
          excess: 0,
          precision: "estimated",
        },
      },
    },
  };
}

test("reservation display distinguishes a sent cap from a matching native proof", () => {
  const unknown = { status: "unknown", reason: "missing" };
  expect(contextView(projection("sent_cap", unknown)).inspection?.output_reservation).toBe(
    "sent_cap",
  );
  const proof = {
    status: "supported",
    value: { tokens: 20, accounting: "total_generated_tokens" },
  };
  expect(contextView(projection("native_maximum", proof)).inspection?.output_reservation).toBe(
    "native_maximum",
  );
  expect(() => contextView(projection("native_maximum", unknown))).toThrow();
  expect(() => contextView(projection("unsupported", proof))).toThrow();
  expect(() => contextView(projection("sent_cap", unknown, 19))).toThrow();
  for (const value of [
    { tokens: 19, accounting: "total_generated_tokens" },
    { tokens: 20, accounting: "visible_text" },
  ]) {
    expect(() =>
      contextView(projection("native_maximum", { status: "supported", value })),
    ).toThrow();
  }
});
