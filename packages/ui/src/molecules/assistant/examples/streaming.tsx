import { useEffect, useState } from "react";

import { AssistantLoader } from "../reasoning";
import { StreamingText } from "../streaming-text";

/** Synthetic controller for the assistant-ui host-driven word stream. */
export function ExampleStreamingText({ text }: { text: string }) {
  const [count, setCount] = useState(0);
  const words = text.split(" ").length;
  useEffect(() => {
    const timer = setInterval(() => {
      setCount((current) => {
        const next = Math.min(current + 1, words);
        if (next === words) {
          clearInterval(timer);
        }
        return next;
      });
    }, 140);
    return () => {
      clearInterval(timer);
    };
  }, [words]);
  return <StreamingText segments={[{ text }]} count={count} streaming={count < words} />;
}

/** Decorative demo cadence belongs to this fixture; it represents no execution progress. */
export function ExampleAssistantLoader({ label }: { label: string }) {
  const [tick, setTick] = useState(0);
  useEffect(() => {
    const timer = setInterval(() => {
      setTick((current) => current + 1);
    }, 100);
    return () => {
      clearInterval(timer);
    };
  }, []);
  return <AssistantLoader label={label} tick={tick} />;
}
