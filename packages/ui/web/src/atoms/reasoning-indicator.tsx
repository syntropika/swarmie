export type ReasoningIndicatorProps = {
  levels: readonly string[];
  value: string;
  className?: string;
};

/** Relative position within one model's ordered levels, not a comparable token budget. */
export function ReasoningIndicator({
  levels,
  value,
  className = "size-5 shrink-0",
}: ReasoningIndicatorProps) {
  const index = levels.indexOf(value);
  const progress =
    index === -1 || ["none", "zero", "off"].includes(value.toLowerCase())
      ? 0
      : ((index + 1) / levels.length) * 100;
  return (
    <svg
      aria-hidden="true"
      data-slot="thinking-ring"
      viewBox="0 0 32 32"
      fill="none"
      className={className}
    >
      <circle
        cx="16"
        cy="16"
        r="15"
        strokeWidth="2"
        vectorEffect="non-scaling-stroke"
        className="stroke-control-indicator"
      />
      {progress > 0 ? (
        <circle
          cx="16"
          cy="16"
          r="15"
          pathLength="100"
          strokeWidth="2"
          vectorEffect="non-scaling-stroke"
          strokeDasharray={`${progress} 100`}
          strokeLinecap="round"
          transform="rotate(-90 16 16)"
          className="stroke-brand-accent"
        />
      ) : null}
    </svg>
  );
}
