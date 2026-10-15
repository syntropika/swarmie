import { describe, expect, test } from "bun:test";
import { rejects } from "node:assert/strict";

import type { RuntimeEvent } from "@swarmie/contracts";

import { connectionSchema } from "~/src/connection/settings";
import { appendEvents, projectTranscript } from "~/src/conversations/journal";
import { createOutbox } from "~/src/conversations/pending-input";
import { scheduleInstant } from "~/src/routines/contracts";

const transcriptCopy = {
  user: "You",
  agent: "Agent",
  runtime: "Runtime",
  attachment: "[Attachment]",
  agentError: "Agent error",
};

function delta(sequence: number, text: string, sessionId = "session-a"): RuntimeEvent {
  return {
    type: "text.delta",
    data: { text, item_id: "message-a" },
    sequence,
    sessionId,
    source: "runtime",
    timestampMs: sequence,
    turnId: "turn-a",
  };
}

describe("mobile trust boundaries", () => {
  test("requires HTTPS and keeps credentials outside URLs", () => {
    for (const url of [
      "",
      "runtime",
      "http://runtime.example.com",
      "https://user:secret@runtime.example.com",
      "https://runtime.example.com?token=secret",
      "https://runtime.example.com#secret",
    ]) {
      expect(connectionSchema.safeParse({ url, token: "device-token" }).success).toBe(false);
    }
    expect(
      connectionSchema.safeParse({ url: "https://runtime.example.com/rpc", token: "device-token" })
        .success,
    ).toBe(true);
  });

  test("journal replay deduplicates without duplicating streamed text", () => {
    const events = appendEvents([delta(1, "Hello")], [delta(1, "Hello"), delta(2, " world")]);
    expect(projectTranscript(events, transcriptCopy).rows).toEqual([
      { id: "1", role: "Agent", text: "Hello world" },
    ]);
    expect(() => appendEvents(events, [delta(2, "other")])).toThrow("Conflicting");
    expect(() => appendEvents(events, [delta(3, "other", "session-b")])).toThrow("identity");
  });

  test("timezone offsets identify the same instant and ambiguous local times fail", () => {
    expect(scheduleInstant("2099-01-01T10:00:00+01:00")).toBe(
      scheduleInstant("2099-01-01T09:00:00Z"),
    );
    expect(() => scheduleInstant("2099-01-01T09:00")).toThrow("timezone");
    expect(() => scheduleInstant("2000-01-01T09:00Z")).toThrow("future");
  });

  test("non-streaming harness responses render and final snapshots do not duplicate a stream", () => {
    const snapshot: RuntimeEvent = {
      ...delta(2, "unused"),
      type: "message.assistant",
      data: { content: [{ type: "text", text: "Final answer" }] },
    };
    expect(projectTranscript([snapshot], transcriptCopy).rows[0]?.text).toBe("Final answer");
    expect(
      projectTranscript([delta(1, "Final answer"), snapshot], transcriptCopy).rows,
    ).toHaveLength(1);
  });

  test("unresolved input survives a new outbox owner and prevents silent resubmission", async () => {
    const values = new Map<string, string>();
    const storage = {
      getItem: async (key: string) => values.get(key) ?? null,
      setItem: async (key: string, value: string) => {
        values.set(key, value);
      },
      removeItem: async (key: string) => {
        values.delete(key);
      },
    };
    const first = createOutbox(storage, () => "submission-a");
    await first.prepareInput("store-a", "session-a", "Check work", []);
    const restored = createOutbox(storage, () => "submission-b");
    const pending = await restored.loadPending("store-a", "session-a");
    expect(pending?.key).toBe("submission-a");
    await rejects(restored.prepareInput("store-a", "session-a", "Check again", []), /Resolve/u);
    expect(await restored.loadPending("store-b", "session-a")).toBeNull();
    await restored.clearPending("store-a", "session-a");
    const retry = await restored.prepareInput("store-a", "session-a", "Intentional retry", []);
    expect(retry.key).toBe("submission-b");
  });
});
