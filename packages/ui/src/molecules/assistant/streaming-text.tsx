import type { ComponentPropsWithRef } from "react";

import { cn } from "../../classes";

export type StreamingSegment = { text: string; mono?: boolean };

function segmentWords(segments: readonly StreamingSegment[]) {
  const words: { offset: number; text: string; mono: boolean }[] = [];
  let offset = 0;
  for (const segment of segments) {
    for (const word of segment.text.split(" ")) {
      words.push({ offset, text: word, mono: segment.mono === true });
      offset += word.length + 1;
    }
  }
  return words;
}

/** Host-controlled word arrival and 700ms settling, distinct from AICSS's character reveal. */
export function StreamingText({
  segments,
  count,
  streaming,
  className,
  ...props
}: ComponentPropsWithRef<"p"> & {
  segments: readonly StreamingSegment[];
  count: number;
  streaming: boolean;
}) {
  const shown = segmentWords(segments).slice(0, Math.max(0, Math.floor(count)));
  return (
    <p
      {...props}
      aria-busy={streaming}
      className={cn("min-h-34 max-w-sm text-content leading-content text-pretty", className)}
    >
      {shown.map((word, index) => (
        <span key={word.offset} className="motion-safe:animate-assistant-word">
          <span
            className={cn(
              "transition-colors duration-700 motion-reduce:transition-none",
              streaming && shown.length - index <= 2 && "text-primary",
              word.mono && "rounded-control bg-surface-muted px-1.5 py-0.5 font-mono text-code",
            )}
          >
            {word.text}
          </span>{" "}
        </span>
      ))}
      {streaming && shown.length > 0 ? (
        <span
          aria-hidden="true"
          className="ml-0.5 inline-block h-4 w-0.5 rounded-pill bg-primary align-text-bottom motion-safe:animate-pulse"
        />
      ) : null}
    </p>
  );
}
