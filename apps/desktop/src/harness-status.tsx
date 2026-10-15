import { TooltipContent } from "~/src/components/ui/tooltip";

const states = [
  {
    id: "idle",
    label: "Idle",
    description: "Ready for your next message",
    color: "bg-harness-idle",
  },
  {
    id: "starting",
    label: "Starting",
    description: "Opening an agent session",
    color: "bg-harness-working",
  },
  {
    id: "running",
    label: "Running",
    description: "Working on your request",
    color: "bg-harness-working",
  },
  {
    id: "needs_input",
    label: "Needs input",
    description: "Waiting for your answer or approval",
    color: "bg-harness-waiting",
  },
  {
    id: "cancelling",
    label: "Cancelling",
    description: "Stopping the current task",
    color: "bg-harness-waiting",
  },
  {
    id: "failed",
    label: "Failed",
    description: "The task ended with an error",
    color: "bg-danger",
  },
  {
    id: "closed",
    label: "Closed",
    description: "The agent session has ended",
    color: "bg-harness-unavailable",
  },
  {
    id: "disconnected",
    label: "Disconnected",
    description: "The local workspace is not connected",
    color: "bg-harness-unavailable",
  },
];

function currentStatus(state: string, connected: boolean) {
  return states.find((entry) => entry.id === (connected ? state : "disconnected"));
}
export function HarnessStatusDot({ state, connected }: { state: string; connected: boolean }) {
  const current = currentStatus(state, connected);
  return (
    <span
      aria-hidden="true"
      className={`size-2 shrink-0 rounded-full ${current?.color ?? "bg-harness-unavailable"}`}
    />
  );
}
export function HarnessStatus({ state, connected }: { state: string; connected: boolean }) {
  const current = currentStatus(state, connected);
  const label = current?.label ?? "Unknown state";
  return (
    <TooltipContent>
      <div className="max-w-72 py-1">
        <p className="font-medium">{label}</p>
        <p className="text-muted mb-3 text-xs">
          {current?.description ?? "The agent has not reported a recognized state"}
        </p>
        <ul className="grid gap-1.5 text-xs">
          {[
            ...states.filter((entry) => !["starting", "closed", "disconnected"].includes(entry.id)),
            {
              id: "unavailable",
              label: "Closed / Disconnected",
              description: "Session ended or local workspace disconnected",
              color: "bg-harness-unavailable",
            },
          ].map((entry) => (
            <li key={entry.id} className="flex items-center gap-2">
              <span aria-hidden="true" className={`size-2 shrink-0 rounded-full ${entry.color}`} />
              <span>
                <span className="font-medium">{entry.label}</span> — {entry.description}
              </span>
            </li>
          ))}
        </ul>
      </div>
    </TooltipContent>
  );
}
