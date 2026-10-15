import { useEffect, useRef } from "react";

/** Runtime staggering is measured choreography; Tailwind owns layout and appearance. */
export function useImageDotMotion(active: boolean) {
  const ref = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    const element = ref.current;
    if (element === null || !active) {
      return undefined;
    }
    const preference = window.matchMedia("(prefers-reduced-motion: reduce)");
    let animations: Animation[] = [];
    const update = () => {
      for (const animation of animations) {
        animation.cancel();
      }
      animations = [];
      if (preference.matches) {
        return;
      }
      animations = [...element.children].map((dot, index) =>
        dot.animate([{ opacity: 0.25 }, { opacity: 1 }, { opacity: 0.25 }], {
          duration: 2000,
          delay: (Math.floor(index / 8) + (index % 8)) * 90,
          iterations: Infinity,
          easing: "ease-in-out",
        }),
      );
    };
    update();
    preference.addEventListener("change", update);
    return () => {
      preference.removeEventListener("change", update);
      for (const animation of animations) {
        animation.cancel();
      }
    };
  }, [active]);
  return ref;
}
