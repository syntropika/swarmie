import type { ComponentPropsWithRef } from "react";

import { Icon } from "../../atoms/icon";
import { cn } from "../../classes";
import { ConversationArrival } from "./conversation-motion";
import { SyntaxTokens } from "./renderers";

export type FileDiffRow = {
  id: string;
  kind: "context" | "added" | "removed";
  oldLine: number | null;
  newLine: number | null;
  text: string;
};

const rowSigns = { context: " ", added: "+", removed: "−" };

const rowClasses = {
  context: "text-text-muted",
  added: "bg-success-soft text-success before:bg-success",
  removed: "bg-danger-soft text-danger before:assistant-deletion-marker",
};

/** Unified source diff with paired line numbers, semantic signs, and safe syntax highlighting. */
export function FileDiff({
  file,
  rows,
  language = "javascript",
  label,
  describeRow,
  className,
  ...props
}: ComponentPropsWithRef<"section"> & {
  file: string;
  rows: readonly FileDiffRow[];
  language?: string;
  label: string;
  describeRow: (row: FileDiffRow) => string;
}) {
  const added = rows.filter((row) => row.kind === "added").length;
  const removed = rows.filter((row) => row.kind === "removed").length;
  return (
    <section
      {...props}
      aria-label={label}
      className={cn(
        "min-w-0 overflow-hidden rounded-panel bg-surface-muted font-mono text-code leading-code",
        className,
      )}
    >
      <header className="flex items-center gap-2 px-3 py-2.5">
        <Icon name="file" className="size-4 text-text-muted" />
        <span className="min-w-0 flex-1 truncate">{file}</span>
        <span className="text-success">+{added}</span>
        <span className="text-danger">−{removed}</span>
      </header>
      <section
        aria-label={label}
        // oxlint-disable-next-line jsx-a11y/no-noninteractive-tabindex -- Keyboard users need focus to scroll source lines horizontally.
        tabIndex={0}
        className="overflow-x-auto py-1 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
      >
        <div className="w-max min-w-full">
          {rows.map((row, index) => (
            <ConversationArrival
              delay={index * 60}
              motion="fade"
              key={row.id}
              className={cn(
                "relative grid grid-cols-[2rem_2rem_1.125rem_1fr] before:absolute before:inset-y-0 before:left-0 before:w-0.5",
                rowClasses[row.kind],
              )}
            >
              <span className="sr-only">{describeRow(row)}</span>
              <span aria-hidden="true" className="px-1.5 text-right text-xs tabular-nums">
                {row.oldLine}
              </span>
              <span aria-hidden="true" className="px-1.5 text-right text-xs tabular-nums">
                {row.newLine}
              </span>
              <span aria-hidden="true" className="text-center">
                {rowSigns[row.kind]}
              </span>
              <code className="pr-3 pl-2 whitespace-pre text-ink">
                <SyntaxTokens source={row.text} language={language} tone="surface" />
              </code>
            </ConversationArrival>
          ))}
        </div>
      </section>
    </section>
  );
}
