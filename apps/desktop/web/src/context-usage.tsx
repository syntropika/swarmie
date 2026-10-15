import { Tooltip, TooltipContent } from "~/web/src/components/ui/tooltip";
import type { Thread } from "~/web/src/protocol";

const formatTokens = (value: number) =>
  new Intl.NumberFormat("en", { notation: "compact", maximumFractionDigits: 1 })
    .format(value)
    .toLowerCase();

export function ContextUsage({ context }: { context: Thread["context"] }) {
  const used = context?.usedTokens;
  const window = context?.contextWindowTokens;
  const hasUsed = typeof used === "number" && Number.isFinite(used) && used >= 0;
  const hasWindow = typeof window === "number" && Number.isFinite(window) && window > 0;
  const reported = context?.percent;
  const percent = (() => {
    if (hasUsed && hasWindow) {
      return (used / window) * 100;
    }
    if (typeof reported === "number" && Number.isFinite(reported)) {
      return reported;
    }
    return undefined;
  })();
  const progress = Math.max(0, Math.min(100, percent ?? 0));
  const label =
    percent === undefined
      ? "Context window: usage unavailable"
      : `Context window: ${Math.round(percent)}% full`;
  return (
    <Tooltip>
      <button
        type="button"
        aria-label={label}
        className="text-muted hover:bg-control-hover focus-visible:ring-focus flex size-7 shrink-0 items-center justify-center rounded-full outline-none focus-visible:ring-2"
      >
        <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true" className="-rotate-90">
          <circle
            cx="8"
            cy="8"
            r="5.5"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            className="opacity-30"
          />
          <circle
            cx="8"
            cy="8"
            r="5.5"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            pathLength="100"
            strokeDasharray={`${progress} 100`}
            strokeLinecap="round"
          />
        </svg>
      </button>
      <TooltipContent>
        <div className="py-1 text-center tabular-nums">
          <p className="text-muted">Context window:</p>
          <p>{percent === undefined ? "Usage not reported yet" : `${Math.round(percent)}% full`}</p>
          <p className="mt-1">
            {(() => {
              if (hasUsed && hasWindow) {
                return `${formatTokens(used)} / ${formatTokens(window)} tokens used`;
              }
              if (hasUsed) {
                return `${formatTokens(used)} tokens used · capacity unavailable`;
              }
              return "Waiting for agent usage data";
            })()}
          </p>
        </div>
      </TooltipContent>
    </Tooltip>
  );
}
