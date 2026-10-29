import { codeLanguage, highlightCode } from "@swarmie/ui-web/code";
import type { CodeToken } from "@swarmie/ui-web/code";

import type { DiffLine } from "./diff";

type SyntaxGroup = { before: DiffLine[]; after: DiffLine[] };

/** Separate old/new grammar state, resetting across omitted hunk context. */
export async function highlightDiff(lines: readonly DiffLine[], path: string) {
  const groups: SyntaxGroup[] = [];
  let group: SyntaxGroup = { before: [], after: [] };
  for (const line of lines) {
    if (line.kind === "header") {
      if (group.before.length || group.after.length) {
        groups.push(group);
      }
      group = { before: [], after: [] };
    } else {
      if (line.kind !== "added") {
        group.before.push(line);
      }
      if (line.kind !== "removed") {
        group.after.push(line);
      }
    }
  }
  if (group.before.length || group.after.length) {
    groups.push(group);
  }
  const result = new Map<string, readonly CodeToken[]>();
  await Promise.all(
    groups.flatMap(({ before, after }) =>
      [
        { side: "before", lines: before },
        { side: "after", lines: after },
      ].map(async ({ side, lines: source }) => {
        const tokens = await highlightCode(
          source.map((line) => line.text).join("\n"),
          codeLanguage(path),
        );
        for (const [index, line] of source.entries()) {
          result.set(`${side}:${line.id}`, tokens[index] ?? []);
        }
      }),
    ),
  );
  return result;
}
