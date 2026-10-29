import type { Message } from "@swarmie/plugin-conversations-browser";
import type { ToolDetails } from "@swarmie/plugin-settings-browser";

export type TranscriptRow =
  | { kind: "message"; key: string; message: Message }
  | { kind: "tools"; key: string; messages: readonly Message[] }
  | { kind: "activity"; key: "activity" };

export const transcriptMessageKey = (id: string) => `message:${id}`;
export const transcriptRowKey = (row: TranscriptRow) => row.key;

/** Build display rows without changing canonical messages or inferring tool calls. */
export function transcriptRows(
  messages: readonly Message[],
  mode: ToolDetails,
  activity: boolean,
): TranscriptRow[] {
  const rows: TranscriptRow[] = [];
  for (const message of messages) {
    if (message.role === "tool") {
      if (mode === "hidden") {
        continue;
      }
      const previous = rows.at(-1);
      if (mode === "grouped" && previous?.kind === "tools") {
        previous.messages = [...previous.messages, message];
      } else {
        rows.push({ kind: "tools", key: `tools:${message.id}`, messages: [message] });
      }
    } else {
      if (
        message.text === "" &&
        message.kind === undefined &&
        message.interrupted !== true &&
        message.truncated !== true
      ) {
        continue;
      }
      rows.push({ kind: "message", key: transcriptMessageKey(message.id), message });
    }
  }
  if (activity) {
    rows.push({ kind: "activity", key: "activity" });
  }
  return rows;
}
