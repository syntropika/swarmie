import { createElement, useSyncExternalStore } from "react";

import type { CounterSnapshot, Observation } from "./contracts";

/** Renderer exports receive local observation adapters, never serialized functions. */
export function CounterScreen(props: { readonly model: Observation<CounterSnapshot> }) {
  const snapshot = useSyncExternalStore(props.model.subscribe, props.model.getSnapshot);
  return createElement(
    "section",
    { "data-screen": "counter", className: "space-y-4" },
    createElement(
      "h2",
      { className: "text-2xl font-semibold" },
      "A screen contributed by a plugin",
    ),
    createElement("p", { className: "text-muted" }, "The shell only knows its public surface."),
    createElement(
      "p",
      { id: "count", className: "text-4xl font-semibold" },
      String(snapshot.count),
    ),
    createElement(
      "dl",
      { className: "grid grid-cols-2 gap-3" },
      createElement("dt", {}, "Last handler generation"),
      createElement("dd", { id: "generation" }, snapshot.generation),
      createElement("dt", {}, "Last native runtime result"),
      createElement("dd", { id: "server-result" }, snapshot.serverResult ?? "No action yet"),
    ),
  );
}

/** Deliberate fault used to qualify the contribution error boundary. */
export function BrokenScreen(): never {
  throw new Error("Deliberate spike renderer failure");
}
