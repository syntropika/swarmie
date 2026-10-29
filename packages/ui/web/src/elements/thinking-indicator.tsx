import type { ComponentProps } from "react";

import { ShimmerLabel } from "../../target/assistant-elements/packages/ui/src/components/react/assistant-ui/elements/surfaces";
import { cn } from "../classes";

/** Runtime-neutral Elements activity presentation with shared status semantics and brand tokens. */
export function ThinkingIndicator({
  label,
  elapsed,
  className,
  ...props
}: Omit<ComponentProps<"output">, "children" | "label"> & {
  readonly label: string;
  readonly elapsed?: string;
}) {
  return (
    <output
      data-slot="thinking-indicator"
      aria-live="polite"
      aria-atomic="true"
      className={cn(
        "flex min-w-0 flex-wrap items-center gap-2.5 text-ui text-text-muted",
        className,
      )}
      {...props}
    >
      <span
        aria-hidden="true"
        className="size-1.5 shrink-0 animate-pulse rounded-full bg-brand-accent motion-reduce:animate-none"
      />
      <ShimmerLabel>{label}</ShimmerLabel>
      {elapsed === undefined ? null : (
        <span className="shrink-0 text-chrome tabular-nums">{elapsed}</span>
      )}
    </output>
  );
}
