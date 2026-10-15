import { useEffect, useState } from "react";

import { Button } from "../../../atoms/button";
import { ThinkingReasoning } from "../thinking-reasoning";

const sentences = [
  "Reading the request and the current selection, then locating the shared component contracts.",
  "Checking how each control exposes its label, value, focus, and disabled state to the caller.",
  "Tracing the animation timing and confirming that reduced motion retains every interaction.",
  "Planning the implementation with shared tokens and compact, rounded controls throughout.",
  "Scanning the existing stories so the changes remain covered at desktop and mobile sizes.",
  "Preparing the preview with focused interaction checks and a complete state walkthrough.",
].map((text, index) => ({ id: `sentence-${index}`, text }));

/** Each replay owns a fresh stream and clears the previous run before any new text arrives. */
export function ExampleReasoningStream() {
  const [state, setState] = useState<{
    run: number;
    count: number;
    phase: "thinking" | "completed";
  }>({ run: 0, count: 0, phase: "thinking" });
  const { run, phase } = state;
  useEffect(() => {
    if (phase === "completed") {
      return undefined;
    }
    const timers = sentences.map((_, index) =>
      setTimeout(
        () => {
          setState((current) => (current.run === run ? { ...current, count: index + 1 } : current));
        },
        (index + 1) * 800,
      ),
    );
    timers.push(
      setTimeout(
        () => {
          setState((current) =>
            current.run === run ? { ...current, phase: "completed" } : current,
          );
        },
        sentences.length * 800 + 360,
      ),
    );
    return () => {
      for (const timer of timers) {
        clearTimeout(timer);
      }
    };
  }, [run, phase]);
  return (
    <div className="space-y-3">
      <ThinkingReasoning
        key={state.run}
        sentences={sentences.slice(0, state.count)}
        phase={state.phase}
        thinkingLabel="Thinking…"
        completedLabel="Thought for 5s"
        toggleLabel="Toggle thought"
      />
      <div className="flex flex-wrap gap-2">
        <Button
          variant="secondary"
          size="sm"
          onClick={() => {
            setState((current) => ({ run: current.run + 1, count: 0, phase: "thinking" }));
          }}
        >
          Start thinking
        </Button>
        <Button
          variant="ghost"
          size="sm"
          disabled={state.phase === "completed"}
          onClick={() => {
            setState((current) => ({ ...current, count: sentences.length, phase: "completed" }));
          }}
        >
          Finish thinking
        </Button>
      </div>
    </div>
  );
}
