import type { ComponentPropsWithRef } from "react";

import { cn } from "../classes";

export type SwarmieMarkProps = ComponentPropsWithRef<"svg"> & {
  tone?: "primary" | "inverse" | "on-primary" | "footer";
};

const toneClasses = {
  primary: "text-primary",
  inverse: "text-inverse-text",
  footer: "text-footer-text",
  "on-primary": "text-on-primary",
} as const;

/** A decorative brand mark with foregrounds for ordinary, inverse, primary, and footer surfaces. */
export function SwarmieMark({ className, tone = "primary", ...props }: SwarmieMarkProps) {
  return (
    <svg
      aria-hidden="true"
      viewBox="0 0 32 32"
      fill="none"
      className={cn("shrink-0", toneClasses[tone], className)}
      {...props}
    >
      <path d="M5 7h10a5 5 0 0 1 5 5v3H10a5 5 0 0 1-5-5Z" fill="currentColor" />
      <path d="M12 17h10a5 5 0 0 1 5 5v3H17a5 5 0 0 1-5-5Z" fill="currentColor" />
      <path d="M24 5v5M21.5 7.5h5M7 21v5M4.5 23.5h5" stroke="currentColor" strokeWidth="1.2" />
    </svg>
  );
}
