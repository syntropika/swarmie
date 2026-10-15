import { useEffect, useState } from "react";
import type { ComponentPropsWithRef } from "react";

import { cn } from "../../classes";

/** Reference cadence: two code points every 9ms, a steady active caret, then a 1s blink. */
export function StreamingTextReveal({
  text,
  className,
  ...props
}: ComponentPropsWithRef<"p"> & { text: string }) {
  const [shown, setShown] = useState("");
  useEffect(() => {
    const preference = window.matchMedia("(prefers-reduced-motion: reduce)");
    const points = [
      ...new Intl.Segmenter(undefined, { granularity: "grapheme" }).segment(text),
    ].map(({ segment }) => segment);
    let count = 0;
    let timer: ReturnType<typeof setInterval> | undefined;
    const start = () => {
      if (timer !== undefined) {
        clearInterval(timer);
      }
      if (preference.matches) {
        setShown(text);
        return;
      }
      timer = setInterval(() => {
        count += 2;
        setShown(points.slice(0, count).join(""));
        if (count >= points.length && timer !== undefined) {
          clearInterval(timer);
        }
      }, 9);
    };
    start();
    preference.addEventListener("change", start);
    return () => {
      if (timer !== undefined) {
        clearInterval(timer);
      }
      preference.removeEventListener("change", start);
    };
  }, [text]);
  const streaming = shown !== text;
  return (
    <p
      {...props}
      aria-busy={streaming}
      className={cn(
        "max-w-prose text-content leading-content break-words whitespace-pre-wrap",
        className,
      )}
    >
      {shown}
      <span
        aria-hidden="true"
        className={cn(
          "ml-0.5 inline-block h-[1.05em] w-2 bg-ink align-text-bottom",
          !streaming && "motion-safe:animate-assistant-caret",
        )}
      />
    </p>
  );
}
