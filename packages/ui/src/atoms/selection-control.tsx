import type { ComponentPropsWithRef } from "react";

import { cn } from "../classes";
import { Icon } from "./icon";

export type SelectionControlProps = Omit<ComponentPropsWithRef<"input">, "type" | "size"> & {
  type: "checkbox" | "radio";
};

/** Rounded native choices retain form submission, radio grouping, and keyboard behavior. */
export function SelectionControl({ type, className, ...props }: SelectionControlProps) {
  return (
    <span className={cn("relative inline-flex size-4 shrink-0 align-middle", className)}>
      <input
        {...props}
        type={type}
        className="peer m-0 size-4 cursor-pointer appearance-none rounded-pill border-0 bg-surface-hover transition-colors duration-150 checked:bg-primary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary disabled:cursor-not-allowed disabled:opacity-50 motion-reduce:transition-none"
      />
      {type === "checkbox" ? (
        <span
          aria-hidden="true"
          className="pointer-events-none absolute inset-0 flex items-center justify-center text-on-primary opacity-0 peer-checked:opacity-100 peer-disabled:opacity-50"
        >
          <Icon name="check" className="size-3" />
        </span>
      ) : (
        <span
          aria-hidden="true"
          className="pointer-events-none absolute inset-1 rounded-pill bg-on-primary opacity-0 peer-checked:opacity-100 peer-disabled:opacity-50"
        />
      )}
    </span>
  );
}
