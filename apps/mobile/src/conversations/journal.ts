import type { PermissionRequest, RuntimeEvent } from "@swarmie/contracts";
import { z } from "zod";

export function appendEvents(current: RuntimeEvent[], next: RuntimeEvent[]): RuntimeEvent[] {
  const sessionId = current[0]?.sessionId ?? next[0]?.sessionId;
  const retained = new Map(current.map((event) => [event.sequence, event]));
  for (const event of next) {
    if (
      event.sessionId !== sessionId ||
      !Number.isSafeInteger(event.sequence) ||
      event.sequence <= 0
    ) {
      throw new Error("Invalid session journal identity");
    }
    const existing = retained.get(event.sequence);
    if (existing && JSON.stringify(existing) !== JSON.stringify(event)) {
      throw new Error("Conflicting journal event");
    }
    retained.set(event.sequence, event);
  }
  return [...retained.values()].toSorted((a, b) => a.sequence - b.sequence);
}

export type TranscriptRow = { id: string; role: string; text: string };

const presentedEvents = [
  "message.assistant",
  "turn.started",
  "message.user",
  "text.delta",
  "message.ended",
  "turn.ended",
  "permission.requested",
  "permission.resolved",
  "session.ended",
  "error",
] as const;
type PresentedEvent = Extract<RuntimeEvent, { type: (typeof presentedEvents)[number] }>;

function isPresented(event: RuntimeEvent): event is PresentedEvent {
  return presentedEvents.some((type) => event.type === type);
}

/** Project canonical events; repeated snapshots must not duplicate streamed text. */
export function projectTranscript(events: RuntimeEvent[]) {
  const rows: TranscriptRow[] = [];
  const pending = new Map<string, PermissionRequest>();
  let response: TranscriptRow | undefined;
  let streamed = false;
  for (const event of events) {
    if (!isPresented(event)) {
      continue;
    }
    switch (event.type) {
      case "turn.started": {
        response = undefined;
        streamed = false;
        break;
      }
      case "message.user": {
        response = undefined;
        streamed = false;
        const text = event.data.content
          .map((part) => (part.type === "text" ? part.data : "[Attachment]"))
          .join("\n");
        rows.push({ id: String(event.sequence), role: "You", text });
        break;
      }
      case "text.delta": {
        streamed = true;
        if (!response) {
          response = { id: String(event.sequence), role: "Agent", text: "" };
          rows.push(response);
        }
        response.text += event.data.text;
        break;
      }
      case "message.assistant": {
        const snapshot = assistantText(event.data);
        if (snapshot !== null && !streamed) {
          rows.push({ id: String(event.sequence), role: "Agent", text: snapshot });
        }
        break;
      }
      case "message.ended":
      case "turn.ended": {
        response = undefined;
        break;
      }
      case "permission.requested": {
        pending.set(event.data.id, event.data);
        break;
      }
      case "permission.resolved": {
        pending.delete(event.data.id);
        break;
      }
      case "session.ended": {
        pending.clear();
        break;
      }
      case "error": {
        rows.push({
          id: String(event.sequence),
          role: "Runtime",
          text: "The agent reported an error. Inspect the runtime before retrying.",
        });
        break;
      }
      default: {
        break;
      }
    }
  }
  return { rows, pending: [...pending.values()] };
}

const assistantSchema = z.union([
  z.object({ text: z.string() }),
  z.object({
    content: z.union([
      z.string(),
      z.array(z.object({ type: z.string(), text: z.string().optional() })),
    ]),
  }),
]);

function assistantText(value: unknown): string | null {
  const parsed = assistantSchema.safeParse(value);
  if (!parsed.success) {
    return null;
  }
  if ("text" in parsed.data) {
    return parsed.data.text;
  }
  const { content } = parsed.data;
  return typeof content === "string"
    ? content
    : content
        .filter((part) => part.type === "text")
        .map((part) => part.text ?? "")
        .join("\n");
}
