import { CodeLine } from "@swarmie/ui-web/code";
import type { CodeToken } from "@swarmie/ui-web/code";
import { useEffect, useMemo, useState } from "react";

import { diffLines, splitDiff } from "./diff";
import type { DiffLine } from "./diff";
import { highlightDiff } from "./diff-syntax";
import type { DiffMode } from "./protocol";

function changeMarker(line: DiffLine | null) {
  if (line?.kind === "added") {
    return "+";
  }
  if (line?.kind === "removed") {
    return "−";
  }
  return "";
}

function Marker({ line }: { line: DiffLine | null }) {
  const marker = changeMarker(line);
  return (
    <td
      data-diff-marker=""
      className="w-6 text-center font-semibold select-none"
      aria-label={line?.kind === "added" || line?.kind === "removed" ? line.kind : undefined}
    >
      {marker}
    </td>
  );
}

function SplitCells({
  line,
  side,
  tokens,
}: {
  line: DiffLine | null;
  side: "before" | "after";
  tokens: readonly CodeToken[] | undefined;
}) {
  return (
    <>
      <td
        data-change={line?.kind}
        data-diff-number=""
        className={`w-12 px-2 text-right tabular-nums select-none ${side === "after" ? "border-l border-border" : ""}`}
      >
        {side === "before" ? line?.oldLine : line?.newLine}
      </td>
      <td
        data-change={line?.kind}
        data-diff-marker=""
        className="w-6 text-center font-semibold select-none"
        aria-label={line?.kind === "added" || line?.kind === "removed" ? line.kind : undefined}
      >
        {changeMarker(line)}
      </td>
      <td data-change={line?.kind} className="overflow-x-auto pr-4">
        <CodeLine tokens={tokens} text={line?.text ?? ""} />
      </td>
    </>
  );
}

export function DiffView({
  text,
  mode,
  path,
  staged,
}: {
  text: string;
  mode: DiffMode;
  path: string;
  staged: boolean;
}) {
  const lines = useMemo(
    () =>
      diffLines(text).filter((line) => {
        if (line.kind !== "header") {
          return true;
        }
        return line.text.length > 0 && !/^(?:diff --git |index |--- |\+\+\+ )/u.test(line.text);
      }),
    [text],
  );
  const rows = useMemo(() => splitDiff(lines), [lines]);
  const [highlighted, setHighlighted] = useState<{
    text: string;
    path: string;
    tokens: Map<string, readonly CodeToken[]>;
  } | null>(null);
  useEffect(() => {
    let active = true;
    highlightDiff(lines, path)
      .then((tokens) => {
        if (active) {
          setHighlighted({ text, path, tokens });
        }
      })
      .catch(() => {
        if (active) {
          setHighlighted(null);
        }
      });
    return () => {
      active = false;
    };
  }, [lines, text, path]);
  const tokens =
    highlighted?.text === text && highlighted.path === path ? highlighted.tokens : null;
  if (!text) {
    return (
      <p className="p-6 text-sm text-muted">
        No text changes in this selection. Binary files and metadata may still have changed.
      </p>
    );
  }
  return (
    <section
      data-git-diff=""
      className="min-h-0 flex-1 overflow-auto"
      aria-label={mode === "split" ? "Side by side diff" : "Unified diff"}
    >
      {mode === "unified" ? (
        <table className="w-full border-collapse font-mono text-xs leading-6 whitespace-pre">
          <thead className="sr-only">
            <tr>
              <th>Old line</th>
              <th>New line</th>
              <th>Change</th>
              <th>Content</th>
            </tr>
          </thead>
          <tbody>
            {lines.map((line) => (
              <tr key={line.id} data-change={line.kind}>
                <td data-diff-number="" className="w-12 px-2 text-right tabular-nums select-none">
                  {line.oldLine}
                </td>
                <td data-diff-number="" className="w-12 px-2 text-right tabular-nums select-none">
                  {line.newLine}
                </td>
                <Marker line={line} />
                <td className="pr-4">
                  <CodeLine
                    tokens={tokens?.get(
                      `${line.kind === "removed" ? "before" : "after"}:${line.id}`,
                    )}
                    text={line.text}
                  />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : (
        <table
          data-git-split=""
          className="w-full table-fixed border-collapse font-mono text-xs leading-6 whitespace-pre"
        >
          <colgroup>
            <col className="w-12" />
            <col className="w-6" />
            <col />
            <col className="w-12" />
            <col className="w-6" />
            <col />
          </colgroup>
          <thead>
            <tr className="sticky top-0 z-10 bg-surface text-left text-muted">
              <th colSpan={3} className="border-b border-border px-4 py-2 font-medium">
                {staged ? "HEAD" : "Index"}
              </th>
              <th colSpan={3} className="border-b border-l border-border px-4 py-2 font-medium">
                {staged ? "Index" : "Working tree"}
              </th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <tr key={`${row.left?.id}:${row.right?.id}`}>
                <SplitCells
                  line={row.left}
                  side="before"
                  tokens={tokens?.get(`before:${row.left?.id ?? ""}`)}
                />
                <SplitCells
                  line={row.right}
                  side="after"
                  tokens={tokens?.get(`after:${row.right?.id ?? ""}`)}
                />
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}
