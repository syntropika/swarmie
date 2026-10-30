import { useLayoutEffect, useRef, useState } from "react";

/** Respond to the actual resizable view, including narrow panels on desktop. */
export function useCompactView() {
  const ref = useRef<HTMLElement>(null);
  const [size, setSize] = useState({ compact: true, short: false });
  useLayoutEffect(() => {
    const element = ref.current;
    if (element === null) {
      return undefined;
    }
    const measure = () => {
      const width = element.clientWidth;
      const short = width >= 640 && element.clientHeight < 400;
      const compact = width < 640 || (width < 1024 && short);
      setSize((current) =>
        current.compact === compact && current.short === short ? current : { compact, short },
      );
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => {
      observer.disconnect();
    };
  }, []);
  return { ref, ...size };
}
