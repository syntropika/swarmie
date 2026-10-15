import { useEffect, useRef } from "react";
import type { ReactNode } from "react";

/** Keyed arrivals are cancelled on removal or when the reader requests reduced motion. */
export function ConversationArrival({
  children,
  delay = 0,
  duration = 300,
  className,
  motion = "slide",
}: {
  children: ReactNode;
  delay?: number;
  duration?: number;
  className?: string;
  motion?: "fade" | "slide" | "menu";
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const preference = matchMedia("(prefers-reduced-motion: reduce)");
    if (preference.matches) {
      return undefined;
    }
    const transform = motion === "menu" ? "translateY(4px) scale(0.95)" : "translateY(4px)";
    const from = motion === "fade" ? { opacity: 0 } : { opacity: 0, transform };
    const animation = ref.current?.animate([from, { opacity: 1, transform: "translateY(0)" }], {
      duration,
      delay,
      fill: "backwards",
      easing: motion === "menu" ? "cubic-bezier(0.23,1,0.32,1)" : "ease",
    });
    const cancel = () => animation?.cancel();
    preference.addEventListener("change", cancel);
    return () => {
      cancel();
      preference.removeEventListener("change", cancel);
    };
  }, [delay, duration, motion]);
  return (
    <div ref={ref} className={className}>
      {children}
    </div>
  );
}
